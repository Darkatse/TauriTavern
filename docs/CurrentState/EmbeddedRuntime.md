# 嵌入式运行时（Embedded Runtime）生命周期管控现状

本文档描述 **当前已经落地** 的“消息内嵌入式运行时（iframe）”生命周期管控机制：它解决什么问题、端到端链路如何工作、明确支持/不支持的边界、以及后续开发最容易踩坑的契约。

> 设计与路线图见：`docs/EmbeddedRuntimeRefactorPlan.md`、`docs/EmbeddedRuntimeRefactorPlan_ER3_DeepDive.md`

---

## 1. 范围与结论

当前“嵌入式运行时（ER）”的管理对象是 **消息内 iframe runtime**，主要来源：

- JS-Slash-Runner（JSR）：`div.TH-render` + iframe（Vue Teleport）
- LittleWhiteBox（LWB）：`.xiaobaix-iframe-wrapper` + iframe（wrapper 插入在 `<pre>` 前）

核心结论是：

> 性能与稳定性的关键不是“少渲染一点 DOM”，而是让 iframe runtime 成为 **宿主可管理资源**：有稳定 slotId、有预算、有 park/hydrate、有自愈；并且消息重渲染不再把它们当成普通 DOM 反复销毁重建。

非目标（明确不做）：

- 面板类 runtime 暂不纳入 park（目前依赖浏览器回收即可）。

---

## 2. 当前架构落点

### 2.1 Manager（预算与状态机）

- 入口：`src/tauri/main/services/embedded-runtime/embedded-runtime-service.js`
  - 创建 manager 并挂到 `globalThis.__TAURITAVERN_EMBEDDED_RUNTIME__`（用于 perf-hud / 调试）。
- 核心实现：`src/tauri/main/services/embedded-runtime/embedded-runtime-manager.js`
  - slot 状态：`cold | active | parked | disposed`
  - reconcile 触发：register/touch/可见性变化（IntersectionObserver）等
  - 预算维度：`maxActiveSlots / maxActiveIframes / maxActiveWeight`

### 2.2 Profiles（兼容 vs 性能）

- `src/tauri/main/services/embedded-runtime/embedded-runtime-profiles.js`
  - `compat` / `mobile-safe`
- `src/tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js`
  - 正式配置：`tauritavern-settings.embedded_runtime_profile = 'off' | 'auto' | 'compat' | 'mobile-safe'`
  - bootstrap mirror：`localStorage tt:embeddedRuntimeProfile`
  - 旧版 `localStorage tt:runtimeProfile` 仅用于迁移

### 2.3 Managed iframe slot（park/hydrate + 软停车）

- slot 实现：`src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js`
  - budget park：替换为 `.tt-runtime-placeholder`（可点击恢复）
  - visibility park：替换为 `.tt-runtime-ghost`（占位但不可交互）
  - cold start：当软停车池无可复用 iframe 时，交还给上游渲染管线重建（避免复用已失效的 `blob:` URL）
- 软停车池：`src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js`
  - 目标：尽量复用 browsing context，避免 iframe 重载/白屏

### 2.4 Runtime detectors（DOM 适配注册）

当前已适配两类消息内 runtime wrapper：

- JSR：`src/tauri/main/adapters/embedded-runtime/js-slash-runner-runtime-adapter.js`
- LWB：`src/tauri/main/adapters/embedded-runtime/littlewhitebox-runtime-adapter.js`

它们都走同一策略：

1) 用 DOM selector 找到 host wrapper
2) 提取 signature（代码文本/`xbHash`/iframe srcdoc 等）
3) 生成稳定 slotId（`jsr:<mesid>:<hash>:<index>` / `lwb:<mesid>:<hash>:<index>`）
4) 用 `createManagedIframeSlot(...)` 注册到 manager

### 2.5 Chat 级 adapter（事件驱动 + 自愈）

- 安装：`src/tauri/main/services/embedded-runtime/install.js`
  - 在 `APP_READY` 后安装 chat 级 adapter
- 核心：`src/tauri/main/adapters/embedded-runtime/chat-embedded-runtime-adapter.js`
  - **事件驱动**：只扫描受影响 message（`*_MESSAGE_RENDERED / MESSAGE_UPDATED / MESSAGE_SWIPED / MORE_MESSAGES_LOADED / CHAT_*`）
  - **局部兜底**：保留一个轻量 `MutationObserver` 处理增量插入/移除
  - **点击恢复**：用户点击 `.tt-runtime-placeholder` 会触发 `manager.invalidate(slotId)`（强制下一轮 reconcile 重新 hydrate）

---

## 3. 端到端链路（现在如何工作）

### 3.1 安装时序

1) `src/tauri/main/bootstrap.js` 先读取 bootstrap mirror；仅当 profile 不是 `off` 时，才在 main ready 后动态导入 `installEmbeddedRuntime()`
2) `installEmbeddedRuntime()` 创建 manager（全局可见）
3) `APP_READY` 事件触发后安装 chat adapters，开始扫描与注册 slot

### 3.2 消息重渲染（ER-3.0：渲染事务）

宿主侧已收敛关键 `.mes_text` 重渲染入口到“消息写入 facade + 渲染事务”：

- `src/scripts/tauri/message/mes-text-write.js`
  - `replaceMesTextHtmlWithRuntimePolicy(mesEl, html)`
  - `off` 时直接恢复普通 `.mes_text` HTML 写入语义
  - 其余 profile 下委托给渲染事务

- `src/tauri/main/adapters/embedded-runtime/message-render-transaction.js`
  - `replaceMesTextHtmlPreservingEmbeddedRuntimes(mesEl, html)`

行为（关键点）：

- 当消息内“前端代码块序列”不变时：
  - 保留 JSR `.TH-render`（原位复用，避免 iframe teardown）
  - 保留 LWB `.xiaobaix-iframe-wrapper`（原位复用，并对新 `<pre>` 写回 `data-xb-final/xb-hash`，避免 LWB 触发重渲染导致 iframe 重载）
- 若序列变化：直接回落到普通 `innerHTML` 更新（允许重建）

### 3.3 超预算/离屏（ER-2：park/hydrate）

reconcile 后 manager 会根据 profile 预算与可见性选择：

- active：保持 iframe 在线
- parked：
  - `budget`：显示点击恢复占位（placeholder）
  - `visibility`：显示不可交互占位（ghost）

### 3.4 第三方破坏性 DOM 操作的自愈（ER-3.2）

当第三方脚本/扩展在 slot host 内 **外部 remove** iframe 时：

1) chat 级 `MutationObserver` 捕获到“slot 内 iframe 被移除”
2) 若该移除不是 ER 自己触发（`data-tt-runtime-managed` 一次性标记），则认为是外部破坏
3) 将移除的 iframe 软停车（保留 browsing context），并注销该 slot（释放 manager 状态）
4) 未来同 slotId 再次被发现/注册时，slot 的 `hydrate()` 会优先取回 parked iframe，从而尽量无感恢复

---

## 4. 已支持的边界

- 支持消息内 iframe runtime 的预算管理与 park/hydrate（JSR + LWB）。
- 支持消息编辑/取消/确认等宿主重渲染流程下的 runtime 保留（尽量避免白屏重载）。
- 支持第三方“移除 iframe”的自愈（软停车 + 重新注册取回）。

---

## 5. 明确不支持 / 当前限制

- 面板类 runtime 不纳入 park（延期项）。
- 渲染事务目前以“前端代码块序列完全相同”为前提；不做复杂 diff/对齐（部分复用可作为后续优化项）。
- 仍无法阻止第三方直接 `.html()` 重写 `.mes_text`；当前策略是依赖 ER-3.2 自愈降低伤害。

---

## 6. 持续开发约束（最容易误改的契约）

1) **不要**在宿主代码里直接对消息 `.mes_text` 做全量 `.html()/empty()+append`：应改为使用渲染事务（否则会重新引入 iframe teardown）。
2) 新增 runtime 适配时：
   - slotId 必须稳定（同一 runtime 在同一 message 内应复用同一个 id）
   - signature 提取要轻量，避免对大块文本做高成本扫描
3) 不要把“自愈”做成大量 try/catch 的吞错链路：当前策略是让错误暴露，方便定位；自愈只处理少数结构化事件（外部移除 iframe）。

---

## 7. 调试与观测

- perf-hud：`src/tauri/main/perf/perf-hud.js`
  - HUD 中 `Runtime:` 行来自 `__TAURITAVERN_EMBEDDED_RUNTIME__.getPerfSnapshot()`
  - 可用 `Ctrl+Alt+P` 打开/关闭；或 `localStorage tt:perf=1` 自动启用
- 调试入口：`globalThis.__TAURITAVERN_EMBEDDED_RUNTIME__`
  - `getPerfSnapshot()` 可直接查看 counters（hydrate/dehydrate/register/unregister 等）
- 常用 DOM 标记：
  - slotId：`data-tt-runtime-slot-id`
  - 移动保护：`data-tt-runtime-moving="1"`（渲染事务搬运 wrapper 时的临时标记）
  - 内部移除标记：`data-tt-runtime-managed="1"`（一次性，避免被误判为外部破坏）
  - LWB 稳定标记：`data-xb-final / data-xb-hash`（避免 LWB 重渲染）
