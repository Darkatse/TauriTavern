# 嵌入式运行时（Embedded Runtime）生命周期管控现状

> 架构状态：**static compatibility only**。本文用于解释仍在运行的兼容机制，不再作为 bounded chat 的目标设计。ChatSurface participant 是新 ownership 路线；显式开启 `chat_virtualization_enabled` 会在 install 前跳过 chat ER，并让内容事务绕过旧 wrapper preservation，禁止两套 owner 同时接管 JSR/LWB wrapper。

本文档描述 **当前已经落地** 的“消息内嵌入式运行时（iframe）”生命周期管控机制：它解决什么问题、端到端链路如何工作、明确支持/不支持的边界、以及后续开发最容易踩坑的契约。

> 新 ownership 路线与接入契约见 `docs/API/ChatSurface.md`。

---

## 1. 范围与结论

当前“嵌入式运行时（ER）”的管理对象是 **消息内 iframe runtime**，主要来源：

- JS-Slash-Runner（JSR）：`div.TH-render` + iframe（Vue Teleport）
- LittleWhiteBox（LWB）：`.xiaobaix-iframe-wrapper` + iframe（wrapper 插入在 `<pre>` 前）

该 legacy 机制当时采用的结论是：

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
  - `dehydrate()` 只在同一 slot owner 内临时 park；`dispose()` 同步销毁 active 与 parked iframe，不允许跨 content/chat owner 复用
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

### 3.2 消息重渲染：渲染事务

宿主侧已收敛关键 `.mes_text` 重渲染入口到“消息写入 facade + 渲染事务”：

- `src/scripts/tauri/message/mes-text-write.js`
  - `replaceMesTextHtmlWithRuntimePolicy(mesEl, html, options)`
  - `off` 时直接恢复普通 `.mes_text` HTML 写入语义
  - 其余 profile 下委托给渲染事务

- `src/tauri/main/adapters/embedded-runtime/message-render-transaction.js`
  - `replaceMesTextHtmlPreservingEmbeddedRuntimes(mesEl, html, options)`

行为（关键点）：

- 新 HTML 只在 detached `.mes_text` staging element 上 parse 一次，再通过 `replaceChildren()` 移动已解析节点提交；不再为比较与写入重复 parse 同一字符串。
- 当消息内“前端代码块序列”不变时：
  - 保留 JSR `.TH-render`（原位复用，避免 iframe teardown）
  - 保留 LWB `.xiaobaix-iframe-wrapper`（原位复用，并对新 `<pre>` 写回 `data-xb-final/xb-hash`，避免 LWB 触发重渲染导致 iframe 重载）
- 若序列变化：提交 staging 中的新节点（允许 runtime 重建）

### 3.3 打开聊天的 frontend source handoff

当且仅当已启用已知第三方 code renderer（JSR 或 LWB）时，两个完整聊天打开入口会显式授权 transient source cover：

- 角色聊天：`getChatResult() -> printMessages(...)`，marker 等待 `CHAT_LOADED`。
- 已有群聊：`getGroupChat() -> printMessages(...)`，marker 等待 `CHAT_CHANGED`。

渲染事务在 message 尚未连接 live DOM 时，按 JSR/LWB 已知接受边界找出 frontend `<pre>`，写入带 release event 值的 `data-tt-frontend-source-handoff`。CSS 使这些节点从首次连接 `#chat` 起不参与可见布局，但节点和完整 `textContent` 仍存在，社区 renderer 继续按原 SillyTavern DOM/event 契约读取。

`adapters/chat-surface/frontend-source-handoff.js` 在 `script.js` 模块初始化时即安装，不再等待 legacy ER 的 `APP_READY` adapter。对应事件到达时，它记录当时包含 marker 的精确 `#chat > .mes` root；下一个 `requestAnimationFrame` 只在这些 root 内重新查询同 event marker并无条件撤销。这样既能跟随晚注册 renderer 对同一楼 `.mes_text` 的同步整体重建，也不会把事件后新增的 message root 纳入旧批次。coordinator 不探测 iframe、wrapper、`hidden!` 或 `xb-*` 私有状态，也不等待 timer；未被 renderer 接管的源码因此恢复普通可见 fallback。

这不是社区 renderer 的 claim API，也不是 batch-exact 事务。它是宿主私有、best-effort、event-scoped 的首次布局遮罩，不改变消息数据、DOM 文本、事件名称或扩展接口。事件值只区分角色与群聊的接管机会，不标识某一次 chat-open；极快的同类聊天切换或更晚注册的跨帧 listener 可能让单次遮罩提前结束并退化到上游性能基线。

### 3.4 超预算与离屏：park/hydrate

reconcile 后 manager 会根据 profile 预算与可见性选择：

- active：保持 iframe 在线
- parked：
  - `budget`：显示点击恢复占位（placeholder）
  - `visibility`：显示不可交互占位（ghost）

### 3.5 第三方破坏性 DOM 操作的自愈

当第三方脚本/扩展在 slot host 内 **外部 remove** iframe 时：

1) chat 级 `MutationObserver` 捕获到“slot 内 iframe 被移除”
2) 若该移除不是 ER 自己触发（`data-tt-runtime-managed` 一次性标记），则认为是外部破坏
3) 将移除的 iframe 软停车（保留 browsing context），并注销该 slot（释放 manager 状态）
4) 未来同 slotId 再次被发现/注册时，slot 的 `hydrate()` 会优先取回 parked iframe，从而尽量无感恢复

---

## 4. 已支持的边界

- 支持消息内 iframe runtime 的预算管理与 park/hydrate（JSR + LWB）。
- 常规 final content 重渲染在前端代码序列不变时仍可保留 legacy wrapper。进入消息编辑则是明确的 content-version 边界：旧 runtime 同步释放，textarea 成为临时内容；取消或确认时按最终内容重新建立 runtime，不再维护第二份隐藏 stash owner。
- 支持第三方“移除 iframe”的自愈（软停车 + 重新注册取回）。
- 支持角色聊天和已有群聊 full-load 的 frontend source handoff；在当前 JSR/LWB 正常事件顺序下，为 renderer 提供接管机会后再揭罩，从而减少巨型源码参与首次可见布局。

---

## 5. 明确不支持 / 当前限制

- 面板类 runtime 不纳入 park（延期项）。
- 渲染事务目前以“前端代码块序列完全相同”为前提；不做复杂 diff/对齐（部分复用可作为后续优化项）。
- 仍无法阻止第三方直接 `.html()` 重写 `.mes_text`；当前策略是依赖自愈机制降低伤害。
- source handoff 只覆盖两个明确的既有聊天 full-load 入口；Show More、普通新增消息、编辑、swipe、streaming 与 regex refresh 不在当前范围。
- 宿主无法从未修改的社区扩展获得严格 claim 信号；指定事件后的单 rAF release 是有界机会窗口，不保证每种 renderer 设置都获得相同的性能收益。
- 当前 marker 以事件类型而非 chat-open 批次标识。快速连续打开同类聊天、运行时晚注册的跨帧 listener 或后台节流可能提前结束单次 cover；这是有意接受的性能型天花板，出现真实主路径证据后再升级为 batch token。
- legacy ER adapter 仍会借用 `MESSAGE_UPDATED` 请求 JSR/LWB cold rebuild；这不是新 ChatSurface 的事件语义，也是 bounded policy 必须禁用/删除 legacy ER owner 的原因之一。

---

## 6. Legacy 维护约束（最容易误改的契约）

这些约束只用于维持现有 static profile；不要新增 detector、parking policy 或 slot abstraction。新的 renderer 生命周期接入应实现 ChatSurface participant，并由 bounded capability gate 证明唯一 ownership。bounded preflight 失败会拒绝打开 epoch，不会重新启用本机制。

1) **不要**在宿主代码里直接对消息 `.mes_text` 做全量 `.html()/empty()+append`：应改为使用渲染事务（否则会重新引入 iframe teardown）。
2) 维护现有 JSR/LWB runtime adapter 时：
   - slotId 必须稳定（同一 runtime 在同一 message 内应复用同一个 id）
   - signature 提取要轻量，避免对大块文本做高成本扫描
3) 不要把“自愈”做成大量 try/catch 的吞错链路：当前策略是让错误暴露，方便定位；自愈只处理少数结构化事件（外部移除 iframe）。
4) 不要根据 `!messageElement.isConnected` 自动启用 source handoff。只有能明确指出后续 release event 的 chat-open 批次可以授权；其他 detached render 路径没有该结算契约。
5) 不要把 source handoff 扩展成社区私有状态探测或 timer/observer 等待器。宿主只负责首次布局遮罩，并在框架事件后的一个 rAF 无条件撤销自己的 marker。
6) `dispose()` 是 slot ownership 的终点，必须同步释放 host 与 parking lot 中的 iframe；不要用 chat epoch、延迟事件或 TTL 替代 terminal cleanup。

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
  - frontend source 临时遮罩：`data-tt-frontend-source-handoff="chatLoaded|chat_id_changed"`（正常只存活到对应事件后的一个 rAF）
