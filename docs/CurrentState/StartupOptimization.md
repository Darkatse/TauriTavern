# 开屏启动优化（Stage-based Boot）现状

本文档描述 **当前已经落地** 的开屏启动优化实现：它解决什么问题、端到端链路如何工作、阶段语义与契约是什么、以及后续开发最容易误改的边界。

> 规划与设计背景见：`docs/StartupOptimizationPlan.md`（本文只写“现在怎么跑”）。

---

## 1. 范围与结论

当前开屏优化的核心落点是把启动从“一个巨大的串行 init”拆成 **可见可点（Shell）→ 核心可用（Core）→ 主应用可交互（Full / APP_READY）**，并同时降低首屏 JS/库解析负担。

已落地的关键点：

- **Shell 先到**：`#preloader` 在 `firstLoadInit()` 的 Shell 阶段就移除，主页可以尽早可见可点。
- **Host Ready 显式等待**：在首次 `/api/*` 访问前等待 `__TAURITAVERN_MAIN_READY__`，确保拦截器/路由已安装，且 Tauri 后端已进入可接收 `AppState` 命令的 readiness 状态。
- **启动并发读取**：设置与启动元数据并行读取、按顺序应用，并复用本次结果。
- **扩展启动分层**：扩展发现可提前后台启动；系统扩展仍在 Full 阶段完成，local/global third-party 扩展延后到 `APP_READY` 后串行激活，从启动关键路径移出。
- **lib.bundle 拆分为 core/optional**：`highlight.js` 通过 `lib.js` 的 async helper 按需加载；上游同步 `/lib.js` ABI 保留在 core bundle。
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
    - 前端用 `waitForTauriMainReady()` 等它，确保首次 `/api/*` 调用前 Host 已就绪，且 Rust `BackendReadiness` 已完成。
  - Host Ready 后只加载不会传递导入主应用的模块。设置入口先注册同步/配对监听，事件呈现与设置 UI 共用一个 `APP_READY` Promise；请求参数面板本身依赖主应用，直接在 `APP_READY` 后导入。

### 2.4 前端启动编排：Shell → Core → Full（保持 APP_READY 语义）

- `src/script.js:firstLoadInit()`
  - **Shell 阶段**
    - `initLocales()` 先于任何可见界面：语言数据是静态资源，不依赖 Host 就绪；首帧界面与启动状态文案都已是用户语言
    - `removePreloader()`（`src/scripts/loader.js`）：移除 `#preloader`
    - 初始化纯前端 UI/DOM handler、基础 patch（不会做 `/api/*`）
  - **Core 阶段**
    - `await waitForTauriMainReady({ failFast: true })`：保证 Host 拦截器与 Rust backend readiness 就绪
    - `/csrf-token` + 并发启动 `/api/bootstrap` 与 `/api/settings/get`
    - `initSecrets()` + `primeSecretStateSnapshot(...)` + `readSecretState()`
    - 默认 slash commands、模型/设置等核心模块初始化
  - **Full 阶段**
    - 按设置、角色、群组、头像的顺序应用读取结果。
    - 扩展（如果启用）：
      - 后台：`startOfflineExtensionsDiscovery()`（可在 Core 阶段提前启动）
      - Full：`activateStartupSystemExtensions({ parallelism })` 只激活系统扩展，并在需要时提前完成 Extras auto-connect
    - 对外事件保持原语义：
      - `event_types.APP_INITIALIZED` → `event_types.APP_READY`
  - **Post-ready（后台）**
    - `initTokenizers()`、`initScrapers()`：在 `APP_READY` 后异步执行（避免阻塞首屏）。
    - 若存在启用中的 third-party 扩展：`activateDeferredThirdPartyExtensions({ parallelism: 1 })` 在首屏完成后后台激活，并在完成后 emit `EXTENSION_SETTINGS_LOADED`。

---

## 3. 阶段语义与全局信号（契约）

### 3.1 内部阶段（UI 体验）

- `Shell`：允许 UI 可见/可点；**不允许依赖扩展已激活**。
- `Core`：允许进行首次 `/api/*` 与核心数据加载；扩展仍可能未激活。
- `Full`：完成主应用可交互所需初始化与系统扩展激活；third-party 扩展可能仍在后台补全。

当前暴露的信号：

- `globalThis.__TAURITAVERN_STARTUP_STAGE__`：由 `firstLoadInit()` 写入（shell/core/full）
- `event_types.APP_READY`：作为“主应用可交互”的稳定语义；晚加载的扩展依赖其 auto-fire 行为完成 ready 钩子

### 3.2 Host 就绪（拦截器、路由与 BackendReadiness）

- `window.__TAURITAVERN_MAIN_READY__`：在 `src/tauri/main/bootstrap.js` 设置
- `waitForTauriMainReady()`：前端轮询等待该 promise 注册并 resolve（`src/scripts/extensions/runtime/tauri-ready.js`）
- Tauri runtime 下，Host 初始化链会在任何需要 `AppState` 的首批工作前调用 `wait_for_backend_ready`，避免依赖 Tauri 的 “state not managed” 错误文本重试。

---

## 4. 设置与 bootstrap 元数据读取

`/api/settings/get` 独立读取设置；`/api/bootstrap` 提供角色、群组、头像和密钥状态等启动元数据。前端并发获取，再按既有顺序应用；本次启动复用读取结果，避免重复请求。

设置与角色是启动必需数据，失败会阻止启动；群组、头像或密钥状态失败时保留可见错误并仅禁用对应能力，不阻止 `APP_READY`。

启动失败只有两个出口：`src/init.js` 加载应用模块失败，或 `firstLoadInit()` 抛错。两处都调用 `showStartupFailure(error)` 显示不可关闭的模态失败对话框（错误、阶段与调用栈，可复制或重新加载），并继续抛出原错误，日志照常记录。展示不能替换原错误：`init.js` 的失败可能来自模块加载本身，所以对话框模块在错误路径之外加载，加载失败时作为第二个错误上报。对话框在 Shadow DOM 中渲染，不受主题与自定义 CSS 影响。

后端采样入口见 [bootstrap_commands.rs](../../src-tauri/crates/tauritavern/src/presentation/commands/bootstrap_commands.rs)。

---

## 5. 扩展加载（离线发现 + 激活批处理）现状

> 详细扩展兼容链路见：`docs/CurrentState/ThirdPartyExtensions.md`

当前策略要点：

- 发现：`startOfflineExtensionsDiscovery()` 等待 Host Ready 后调用 `/api/extensions/discover`，并加载各扩展 `manifest.json`。
- 系统扩展激活：`activateStartupSystemExtensions({ parallelism })`
  - 同 `loading_order` 的扩展分组；组内按 `parallelism` 分块 `Promise.all` 并在 chunk 间 `delay(0)` 主动让出事件循环。
  - Android 设备默认 `parallelism = 1`；其他默认 `parallelism = 2`（见 `src/script.js`）。
- third-party 激活：`activateDeferredThirdPartyExtensions({ parallelism: 1 })`
  - 仅处理 `local/global` 扩展，默认在 `APP_READY` 后执行，避免把大体量 third-party 模块求值放进启动关键路径。
- third-party 兼容（hljs）：
  - third-party 扩展激活前会 `await getHljs()`，确保 `window.hljs` 存在（`src/scripts/extensions.js`）。

---

## 6. lib.core / lib.optional（按需加载）现状

### 6.1 产物与入口

- Rspack 入口：`rspack.config.js`
  - `lib.core` → `src/lib-bundle-core.js` → `src/dist/lib.core.bundle.js`
  - `lib.optional` → `src/lib-bundle-optional.js` → `src/dist/lib.optional.bundle.js`

### 6.2 前端门面（lib.js）

- `src/lib.js`
  - 静态 import core bundle，并 re-export 上游常用库
  - 可选库按需加载：
    - `getHljs()`：动态 import optional bundle；并注册 `stscript` 语言（`src/scripts/slash-commands/stscript-hljs-language.js`）
  - `initLibraryShims()`：将少量库挂到 `window`（用于第三方扩展兼容）；其中 `window._ = lodash` 是正式 ABI，需先于 third-party 扩展模块求值完成

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
- `src/tauri/main/adapters/panel-runtime/top-settings-panel-parking.js`
  - 左栏 pinned 锚点分两类，判据不同：
    - `LEFT_NAV_REQUIRED_ANCHORS`（所有档位，正确性）：面板外代码读取的节点保持在线，包括 `onModelChange` 读取边界值的 `#range_block_openai`，助手读取支持来源的 `#openai_reasoning_effort_block`，以及紧凑预设行所在的 `#openai_api-presets`。
    - `LEFT_NAV_COMPAT_ANCHORS`（仅 `compat`，兼容让步）：`#completion_prompt_manager`，让第三方脚本在抽屉关闭时仍能选中。
  - 世界书抽屉：`compat` 只 park 条目列表 `#world_popup_entries_list`（世界书 DOM 的主体），世界书下拉框、分页和编辑按钮保持在线；`aggressive` 仍 park 整个 `#wi-holder`。世界书模块持有列表节点，正常渲染与停放期间的更新使用同一个目标；重新打开只挂回原节点，不额外刷新。离屏更新仍有渲染计算，但停放期间的重型条目不连接 document，保留 DOM 裁剪收益；列表的分页后测量与条目定位跳过离屏节点。

---

## 8. 可观测性（perf + 状态）

- Perf 开关：`localStorage tt:perf = '1'` 或 URL `?ttPerf=1`
- 标记（部分）：
  - `src/init.js`：`tt:init:*`
  - `src/tauri/main/bootstrap.js`：`tt:tauri:*`
  - `src/script.js`：`tt:startup:shell/core/full` + `tt:startup:ready`
- 运行时提示：
  - `src/scripts/tauri/startup/startup-status-overlay.js`：右下角非阻塞启动状态 overlay（`APP_READY` 后移除）与启动失败对话框（见 §4）
- `pnpm run test:browser` 构建 vendor bundle 并运行 `tests/browser/*.test.mjs`。其中 `startup-order.test.mjs` 验证 Host-ready 模块先加载时主应用仍能正常初始化、早到的事件等待应用就绪后呈现，以及启动失败时对话框显示错误且错误继续上报。该测试组包含在 `pnpm test` 和 `pnpm run check` 中。

---

## 9. 明确支持 / 不支持边界

支持：

- 主页尽早可见可点（Shell 先到）。
- `APP_READY` 表示主应用已可交互；late-loaded third-party 扩展依赖其 auto-fire 语义继续完成初始化。
- third-party 扩展资源同源加载，并提供 `window._`（lodash）与 `window.hljs` 兼容（见上）。

不保证（需要按需扩展白名单/契约）：

- 任何第三方脚本在 **扩展抽屉关闭** 时都能访问到其期望的“扩展设置 DOM”（Panel Runtime 可能 park 子树；目前只对白名单容器强保证）。
