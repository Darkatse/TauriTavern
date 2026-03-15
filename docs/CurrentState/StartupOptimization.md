# 开屏启动优化（Stage-based Boot）现状

本文档描述 **当前已经落地** 的开屏启动优化实现：它解决什么问题、端到端链路如何工作、阶段语义与契约是什么、以及后续开发最容易误改的边界。

> 规划与设计背景见：`docs/StartupOptimizationPlan.md`（本文只写“现在怎么跑”）。

---

## 1. 范围与结论

当前开屏优化的核心落点是把启动从“一个巨大的串行 init”拆成 **可见可点（Shell）→ 核心可用（Core）→ 兼容扩展生态的完全就绪（Full / APP_READY）**，并同时降低首屏 JS/库解析负担。

已落地的关键点：

- **Shell 先到**：`#preloader` 在 `firstLoadInit()` 的 Shell 阶段就移除，主页可以尽早可见可点。
- **Host Ready 显式等待**：在首次 `/api/*` 访问前等待 `__TAURITAVERN_MAIN_READY__`，避免拦截器/路由未安装导致的“偶发首包失败”。
- **bootstrap 快照**：用一次 `/api/bootstrap` 拉齐启动关键数据，并在前端以“prime snapshot”方式避免重复请求（settings/characters/groups/avatars/secret_state）。
- **扩展发现/激活后移 + 并行批处理**：扩展发现可提前后台启动，扩展激活在 Full 阶段完成；同 loading_order 内按 `parallelism` 分块并行激活。
- **lib.bundle 拆分为 core/optional**：重库（如 `highlight.js` / Readability）迁到可选 bundle，通过 `lib.js` 的 async helper 按需加载，减少首屏解析/编译压力。
- **重任务后移**：`initTokenizers()`、`initScrapers()` 在 `APP_READY` 后、两次 paint 之后后台启动。

---

## 2. 端到端链路（从 index.html 到 APP_READY）

### 2.1 HTML 入口与预加载遮罩

- `src/index.html`
  - 首屏遮罩：`<div id="preloader"></div>`
  - 模块入口：`<script type="module" src="init.js"></script>`
  - 约束：不使用 modulepreload（移动端 WebView 上曾有缓存/预加载失败问题）。

### 2.2 Bootloader：分层 import + Android import 重试

- `src/init.js`
  - 设置：`window.__TAURI_RUNNING__ = true`，并计算 `globalThis.__TAURITAVERN_PERF_ENABLED__`
  - 通过 `importWithRetry()` 依次加载：
    1) `./lib.js`（静态依赖 `src/dist/lib.core.bundle.js`）
    2) `./tauri-main.js`（安装 Host Kernel：路由/拦截器/ABI）
    3) `./script.js`（SillyTavern 主前端）
  - 目的：在 Android WebView 首启 I/O 抖动场景下提高模块加载确定性。

### 2.3 Host Kernel：把同源 /api 变成可路由端点

- `src/tauri/main/bootstrap.js`
  - 安装 `window.__TAURITAVERN__`（稳定 Host ABI）：invoke broker / 资源路径 helpers 等
  - Patch：
    - `fetch` 拦截（同源、命中路由表则走 `router.handle(...)`）
    - `jQuery.ajax` 拦截（同源、命中路由表则转发）
    - same-origin iframe/window 的补丁（保证扩展/脚本在 iframe 内也能命中路由/下载桥）
  - 设置 readiness：
    - `window.__TAURITAVERN_MAIN_READY__ = readyPromise`
    - 前端用 `waitForTauriMainReady()` 等它，确保首次 `/api/*` 调用前 Host 已就绪。

### 2.4 前端启动编排：Shell → Core → Full（保持 APP_READY 语义）

- `src/script.js:firstLoadInit()`
  - **Shell 阶段**
    - `removePreloader()`（`src/scripts/loader.js`）：移除 `#preloader`
    - 初始化纯前端 UI/DOM handler、基础 patch（不会做 `/api/*`）
  - **Core 阶段**
    - `await waitForTauriMainReady()`：保证 Host 拦截器就绪
    - `/csrf-token` + 并发启动 `fetch('/api/bootstrap')`
    - `initSecrets()` + `primeSecretStateSnapshot(...)` + `readSecretState()`
    - `initLocales()`、默认 slash commands、模型/设置等核心模块初始化
  - **Full 阶段**
    - 应用 settings/角色/群组/头像快照：
      - `applySettingsSnapshot(bootstrap.settings)`
      - `applyCharactersSnapshot(bootstrap.characters)`
      - `applyGroupsSnapshot(bootstrap.groups)`
      - `primeUserAvatarsSnapshot(bootstrap.avatars)` + `getUserAvatars(...)`
    - 扩展（如果启用）：
      - 后台：`startOfflineExtensionsDiscovery()`（可在 Core 阶段提前启动）
      - Full：`activateOfflineExtensions({ parallelism })` 完成激活
    - 对外事件保持原语义：
      - `event_types.APP_INITIALIZED` → `event_types.APP_READY`
  - **Post-ready（后台）**
    - `initTokenizers()`、`initScrapers()`：在 `APP_READY` 后异步执行（避免阻塞首屏）。

---

## 3. 阶段语义与全局信号（契约）

### 3.1 内部阶段（UI 体验）

- `Shell`：允许 UI 可见/可点；**不允许依赖扩展已激活**。
- `Core`：允许进行首次 `/api/*` 与核心数据加载；扩展仍可能未激活。
- `Full`：执行扩展激活与可选模块补全；完成后才 emit `APP_READY`（兼容扩展生态假设）。

当前暴露的信号：

- `globalThis.__TAURITAVERN_STARTUP_STAGE__`：由 `firstLoadInit()` 写入（shell/core/full）
- `event_types.APP_READY`：仍作为“完全就绪”的稳定语义（扩展生态依赖点）

### 3.2 Host 就绪（拦截器与路由）

- `window.__TAURITAVERN_MAIN_READY__`：在 `src/tauri/main/bootstrap.js` 设置
- `waitForTauriMainReady()`：前端轮询等待该 promise 注册并 resolve（`src/scripts/extensions/runtime/tauri-ready.js`）

---

## 4. bootstrap 快照（/api/bootstrap）现状

### 4.1 前端消费

- `src/script.js`：`fetch('/api/bootstrap', { method: 'POST' })`
- 负载（当前使用字段）：
  - `settings`：直接喂给 `applySettingsSnapshot(...)`
  - `characters`：直接喂给 `applyCharactersSnapshot(...)`
  - `groups`：直接喂给 `applyGroupsSnapshot(...)`
  - `avatars`：喂给 `primeUserAvatarsSnapshot(...)`
  - `secret_state`：喂给 `primeSecretStateSnapshot(...)`

### 4.2 Tauri 主进程路由

- `src/tauri/main/routes/bootstrap-routes.js`
  - `router.post('/api/bootstrap', ...)`
  - `context.safeInvoke('get_bootstrap_snapshot')`
  - 角色做一次 `context.normalizeCharacter(...)` 后返回 JSON

### 4.3 Rust 命令（并发采样）

- `src-tauri/src/presentation/commands/bootstrap_commands.rs:get_bootstrap_snapshot`
  - 使用 `tokio::try_join!` 并发获取：settings / characters / groups / avatars / secret_state
  - 目的：减少启动关键路径的串行 I/O 等待。

---

## 5. 扩展加载（离线发现 + 激活批处理）现状

> 详细扩展兼容链路见：`docs/CurrentState/ThirdPartyExtensions.md`

当前策略要点：

- 发现：`startOfflineExtensionsDiscovery()` 等待 Host Ready 后调用 `/api/extensions/discover`，并加载各扩展 `manifest.json`。
- 激活：`activateOfflineExtensions({ parallelism })`
  - 同 `loading_order` 的扩展分组；组内按 `parallelism` 分块 `Promise.all` 并在 chunk 间 `delay(0)` 主动让出事件循环。
  - Android 设备默认 `parallelism = 1`；其他默认 `parallelism = 2`（见 `src/script.js`）。
- third-party 兼容（hljs）：
  - third-party 扩展激活前会 `await getHljs()`，确保 `window.hljs` 存在（`src/scripts/extensions.js`）。

---

## 6. lib.core / lib.optional（按需加载）现状

### 6.1 产物与入口

- webpack 入口：`webpack.config.js`
  - `lib.core` → `src/lib-bundle-core.js` → `src/dist/lib.core.bundle.js`
  - `lib.optional` → `src/lib-bundle-optional.js` → `src/dist/lib.optional.bundle.js`

### 6.2 前端门面（lib.js）

- `src/lib.js`
  - 静态 import core bundle，并 re-export 上游常用库
  - 可选库按需加载：
    - `getHljs()`：动态 import optional bundle；并注册 `stscript` 语言（`src/scripts/slash-commands/stscript-hljs-language.js`）
    - `getReadability()`：动态 import optional bundle
  - `initLibraryShims()`：将少量库挂到 `window`（用于第三方扩展兼容）

### 6.3 代码高亮（延迟执行）

- `src/scripts/tauri/perf/code-highlight-coordinator.js`
  - 通过 `IntersectionObserver` + `requestIdleCallback` 做“近视区”高亮
  - 内部通过 `getHljs()` 拉起可选 bundle（不进入 Shell/Core 静态依赖图）

---

## 7. Panel Runtime 的扩展 DOM “停车”与兼容白名单

Panel Runtime 会在 `APP_READY` 后安装，用于在抽屉关闭时把部分面板 DOM 子树 park 到 `DocumentFragment`，减少低端设备的 DOM/observer 压力。

已知兼容点：

- `src/tauri/main/adapters/panel-runtime/extensions-subtree-gates.js`
  - 当前对扩展设置容器启用“子树 gate + park/hydrate”
  - **白名单永远保持连接**：`regex_container`、`qr_container`
    - 目的：避免 SPresets 等脚本在抽屉关闭时找不到 `#saved_regex_scripts` 触发 `MutationObserver.observe(target not Node)`。

---

## 8. 可观测性（perf + 状态）

- Perf 开关：`localStorage tt:perf = '1'` 或 URL `?ttPerf=1`
- 标记（部分）：
  - `src/init.js`：`tt:init:*`
  - `src/tauri/main/bootstrap.js`：`tt:tauri:*`
  - `src/script.js`：`tt:startup:shell/core/full` + `tt:startup:ready`
- 运行时提示：
  - `src/scripts/tauri/startup/startup-status-overlay.js`：右下角非阻塞启动状态 overlay（`APP_READY` 后移除）

---

## 9. 明确支持 / 不支持边界

支持：

- 主页尽早可见可点（Shell 先到）。
- `APP_READY` 仍表示“扩展已激活 + 全部初始化完成”（最大化扩展生态兼容）。
- third-party 扩展资源同源加载与 `window.hljs` 兼容（见上）。

不保证（需要按需扩展白名单/契约）：

- 任何第三方脚本在 **扩展抽屉关闭** 时都能访问到其期望的“扩展设置 DOM”（Panel Runtime 可能 park 子树；目前只对白名单容器强保证）。

