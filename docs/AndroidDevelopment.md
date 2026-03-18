# TauriTavern Android 端开发说明

本文档记录当前移 Android 端开发中已经踩过的关键问题、根因分析、已落地方案，以及对应的架构改动。目标是避免重复踩坑，并为后续替换官方修复留出清晰迁移路径。

## 1. Android WebView 安全区注入时机竞态

### 1.1 现象

- `#top-settings-holder` 偶发沉入状态栏。
- 现象不稳定：同一版本在不同启动时机下表现不同。
- 简单删除延时重试后，问题明显回归。

### 1.2 根因

根因不是 inset 数值计算本身，而是 **注入时机竞态**：

- Android WebView 启动阶段常经历 `about:blank -> tauri.localhost` 的页面切换。
- 若在 `about:blank` 或 `document.readyState=loading` 时注入 CSS 变量，后续导航会丢失变量。
- 表层看是“safe area 失效”，本质是“注入到了错误上下文或过早上下文”。

参考问题：  
https://github.com/tauri-apps/tauri/issues/14240

### 1.3 当前实现

核心入口仍是 `src-tauri/gen/android/app/src/main/java/com/tauritavern/client/MainActivity.kt`，但职责已拆分为：

- `AndroidInsetsBridge.kt`：系统栏/IME inset 监听与 CSS 变量注入；
- `WebViewReadinessPoller.kt`：页面就绪轮询；
- `ShareIntentParser.kt`：分享 Intent 解析与导入文件持久化；
- `SharePayloadDispatcher.kt`：分享 payload 队列与前端 bridge 分发；
- `MainActivity.kt`：仅保留生命周期编排与模块协作。

- 保留 edge-to-edge 与透明系统栏配置（沉浸基础）；
- 监听系统栏与 IME inset；
- 在 native 侧消费 IME 语义并把底部避让作为 CSS 变量注入；不再让 descendant WebView 将 IME 继续解释为 viewport resize；
- Android native 注入的 CSS 变量（provider 层）：
  - `--tt-inset-top/right/left/bottom`（布局应避开的有效 inset：system bars + cutout 等，沉浸模式下为 0）
  - `--tt-ime-bottom`（输入法可见时的底部 inset）
  - `--tt-base-viewport-height`（无 IME 时的基准 viewport 高度）
- 前端布局消费的 CSS 变量（contract 层）：
  - `--tt-inset-top/right/left/bottom`（有效避让 inset；iOS 由 `env()` 提供，Android 由 native 注入覆盖）
  - `--tt-viewport-bottom-inset`（前端通过 `max()` 合成有效底部 inset）

Android 语义说明（以 contract 层为准）：

- `--tt-inset-*` 表示**当前布局应避开的有效 inset**；
- 非沉浸模式下，它反映 system bars + `displayCutout`（刘海/打孔）的可见/稳定 inset；
- 沉浸模式下，它会回落为 `0`，允许应用顶部 UI 与第三方 fixed 浮层以 full-bleed 方式沉入状态栏区域。
- `--tt-ime-bottom` 是 Android 上唯一的键盘布局信号；WebView 本身不应再把 IME 当作页面 viewport 缩放来源。

注入时序约束：

- 注入前先检查页面就绪：
  - `document.readyState !== 'loading'`
  - `location.href !== 'about:blank'`
  - 未满足时进行有限次短重试。

前端消费变量在：

- `src/style.css`（变量定义与 fallback）
- `src/css/mobile-styles.css`（顶部栏与容器定位使用 contract 变量）

### 1.4 维护原则

- 不要把“就绪态判断”误删为一次性注入。
- 不要把此问题误判为纯 CSS 问题；先验证变量是否被注入到正确页面上下文。
- 若后续 Tauri 官方修复 WebView safe-area 注入时序，可再评估收敛逻辑。

---

## 2. Android 资源访问语义差异（APK assets）

### 2.1 官方语义

Tauri 官方说明：Android 资源位于 APK assets，不是普通文件系统路径，返回值可能为 `asset://localhost/...`，需要通过 fs 插件语义访问。  
https://v2.tauri.app/develop/resources/#android

### 2.2 过去的问题

- 模板文件读取失败（如 popup/template 相关异常）。
- 默认内容索引读取失败（`default/content/index.json` not found）。
- 直接按“普通路径”处理资源导致跨平台行为不一致。

### 2.3 架构改动（资源层收敛）

#### A. 构建期生成资源索引与嵌入映射

`src-tauri/build.rs` 现在会：

- 扫描 `../default/content` 和 `../src/scripts/templates`；
- 生成 `default_content_manifest.json`（默认内容清单）；
- 生成 `embedded_resources.rs`（虚拟路径 -> `include_bytes!` 映射）。

#### B. 运行时统一资源访问入口

`src-tauri/src/infrastructure/assets.rs` 提供统一 API：

- `read_resource_bytes`
- `read_resource_text`
- `read_resource_json`
- `copy_resource_to_file`
- `list_default_content_files_under`

平台策略：

- Android：优先走构建期嵌入资源映射；
- 非 Android：走 `BaseDirectory::Resource` + fs 访问。

#### C. 前后端模板读取解耦

- 后端新增命令：`read_frontend_template`  
  文件：`src-tauri/src/presentation/commands/bridge.rs`
- 前端模板加载改为 Tauri 环境下优先 invoke：  
  文件：`src/scripts/templates.js`

#### D. 默认内容初始化改为“资源 -> 真实文件”复制流程

`src-tauri/src/infrastructure/repositories/file_content_repository.rs` 不再依赖资源目录的直接文件路径语义，改用统一资源接口复制到用户目录。

---

## 3. iOS / Android 应用数据目录解析异常

### 3.1 问题背景

在移动端，Tauri 提供的目录 API 在不同平台/版本可能与预期目录不一致。  
已确认 Android 存在已知问题：`appDataDir/localDataDir` 可能返回内部路径（如 `/data/user/0/...`）而非外部 app 目录（如 `/storage/emulated/0/Android/data/...`）。

### 3.2 当前方案：单点路径解析抽象

新增单点路径解析模块：  
`src-tauri/src/infrastructure/paths.rs`

统一入口：

- `resolve_app_data_dir(app_handle)`

当前行为：

- Android：优先使用 `app_data_dir`，仅当其落在内部目录（如 `/data/user/0/...`）时，自动回退到从 `document_dir` 推导外部 app data 目录；
- 其他平台（含 iOS）：回退到标准 `app_data_dir`。

### 3.3 架构收益

- 所有仓储与应用数据根路径都通过同一函数解析；
- 平台差异被收敛到一个模块，不向业务层扩散；
- 未来若 iOS 出现类似目录异常，可在同一模块增加 `cfg(target_os = "ios")` 分支，不需要修改各仓储。

---

## 4. 与上述问题相关的关键架构调整

### 4.1 基础设施层

- 新增 `infrastructure::assets`（资源读取/复制统一抽象）
- 新增 `infrastructure::paths`（应用数据目录统一抽象）
- `infrastructure::mod.rs` 导出上述模块

### 4.2 应用初始化与数据根目录

- `src-tauri/src/app.rs` 的 `resolve_data_root` / `resolve_log_root` 已改为依赖 `resolve_app_data_dir`

### 4.3 资源协议访问权限

- `src-tauri/src/lib.rs` 在 setup 阶段对 `data_root` 执行：
  - `asset_protocol_scope().allow_directory(&data_root, true)`
- 目的：允许 WebView 通过 asset 协议访问用户数据文件，避免前端资源加载 403。

### 4.4 前端接入点

- `src/scripts/templates.js`：模板读取在 Tauri 环境下走 `invoke('read_frontend_template')`
- `src/css/mobile-styles.css` + `src/style.css`：通过 `--tt-inset-*` 消费布局契约

---

## 5. 后续迁移与清理建议

1. **Tauri 官方修复目录 API 后**  
   `infrastructure/paths.rs` 会自动优先使用修复后的 `app_data_dir`，无需在仓储层做分散修补。

2. **Tauri 官方修复 WebView safe-area 注入后**  
   可评估简化 `MainActivity` 的“页面就绪后注入”逻辑，但必须先验证不会回归 `about:blank` 时序竞态。

3. **新增移动端特性时**  
   优先复用现有单点抽象（`assets.rs` / `paths.rs` / `MainActivity.kt`），避免再次把平台差异扩散到业务代码。

---

## 6. 插件系统（前端）移动端兼容补丁

以下问题仅在 Android 旧 WebView 上高概率出现，桌面端通常不复现。

### 6.1 `*.at is not a function`

现象：

- 第三方插件初始化报错（典型如 `g.at is not a function`）。

根因：

- 插件构建产物使用了较新的 JS API（`Array/String.at`、`toSorted`、`findLastIndex` 等）。
- 旧 Android WebView 缺少这些 API。

已落地方案：

- 在 Tauri mobile 启动期安装运行时兼容层：
  - 实现：`src/tauri/main/compat/mobile/mobile-runtime-compat.js`
  - 入口：`src/tauri/main/bootstrap.js`（仅 Android/iOS UA）
  - 行为：仅补齐缺失 API，且只执行一次；桌面端/移动端 Web 不启用。

### 6.2 插件面板样式大面积失效（如 `TH-custom-tailwind` 布局错乱）

现象：

- 插件 CSS 文件请求成功，但大量样式未生效，界面排布混乱。

根因：

- 旧 Android WebView 对 CSS Cascade Layers（`@layer`）支持不完整。
- 采用 Tailwind v4 打包的插件会把大量规则放在 `@layer` 中，导致整层失效。

已落地方案：

- 在 `src/scripts/extensions/runtime/third-party-runtime.js` 的样式加载链路中：
  - 先探测当前 WebView 是否支持 `@layer`；
  - 不支持时用 `css-tools` 将 `@layer` 规则展平后再注入。

性能策略：

- 支持 `@layer` 的环境走快路径，不转换；
- 能力检测结果缓存；
- 预处理结果走现有样式缓存，避免重复计算。


### 6.3 JS-Slash-Runner 脚本弹窗贴顶（关闭按钮落入状态栏）

现象：

- 某些脚本运行后弹窗顶部被状态栏遮挡，关闭按钮不可点击。

根因：

- 脚本运行时直接向主文档注入 `<style>`；
- 规则常见为 `position: fixed` + `top: 0`，绕过了扩展 CSS 资源链路中的现有修正。

已落地方案：

- 在 Tauri mobile 安装 overlay safe-area 兼容控制器：
  - 实现：`src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js`
  - 入口：`src/tauri/main/bootstrap.js`（仅 Android/iOS UA）
  - 策略：只观察 `document.body` 直接子节点新增/移除；对疑似第三方浮层（`position: fixed` 且顶边贴近 0）按元素级别设置 `top: max(var(--tt-inset-top), <原top>) !important`；并在 `html.style`/`visualViewport` 变化时重新校验。
  - 依赖：`--tt-inset-top` 表示当前布局策略；因此非沉浸模式下会提供顶部避让，沉浸模式下会自然退化为 no-op。

设计约束：

- 仅 Tauri mobile 生效；
- 仅处理 top safe-area 策略，不处理其他边；沉浸模式下不会额外强制避开刘海/状态栏；
- 仅作用于第三方浮层节点，不改写全局 `<style>` 文本与静态主样式文件；
- 明确排除 `body/#sheld/#chat` 等应用核心容器，避免牵连应用本体布局；
- 不侵入第三方扩展资源加载链路（与 `third-party-runtime.js` 解耦）。

---

## 7. Android 返回键分层返回（Back Navigation）

问题：

- Android 端按系统返回键可能直接退出应用（未能按“退一层 UI”关闭弹窗/抽屉/聊天）。
- 不要依赖覆盖 `Activity.onBackPressed()`：在较新的 Android（predictive back / `OnBackInvokedDispatcher`）路径下，Back 分发优先走 `OnBackPressedDispatcher`，`super.onBackPressed()` 也可能绕开子类 override。

当前方案（Native→JS Back Bridge）：

- `MainActivity` 在 `onCreate()` 里向 `onBackPressedDispatcher` 注册回调，并把 Back 交给 `AndroidBackNavigationController`：
  - `src-tauri/gen/android/app/src/main/java/com/tauritavern/client/MainActivity.kt`
  - `src-tauri/gen/android/app/src/main/java/com/tauritavern/client/AndroidBackNavigationController.kt`
- controller 通过 `WebView.evaluateJavascript` 调用前端全局函数：`window.__TAURITAVERN_HANDLE_BACK__()`。
  - JS 返回 `true`：表示已消费 Back（关闭了一层 UI），原生不退出。
  - JS 返回 `false`：表示前端未消费，原生执行 `finish()` 退出。

前端分层关闭策略：

- Back 逻辑集中在 `src/tauri/main/back-navigation.js`，并在 `src/tauri/main/bootstrap.js` 启动早期安装。
- 关闭动作必须复用现有 UI 的关闭入口（点击 close/cancel 或触发既有的“点空白收起”逻辑），避免引入新的状态机。
- “点空白收起”要模拟 `mousedown`（SillyTavern 绑定在 `html` 的 `touchstart/mousedown` 上），仅派发 `click` 不足以关闭抽屉。

维护原则：

- 不修改 auto-generated 的 `src-tauri/gen/android/.../generated/*`，避免升级冲突。
- UI 分层判断与关闭动作只写在 JS；Kotlin 不写 DOM/UI 规则，只做拦截/转发/退出决策。
- 若未来新增/变更 UI 层级，只在 `back-navigation.js` 增加一个分支即可；更详细设计见 `docs/AndroidBackNavigation.md`。

---

## 8. Android WebView 页面 Fullscreen API

问题：

- 桌面端嵌入页面的全屏按钮可正常进入全屏；
- Android 端同一路径报错：`Fullscreen is not supported (TypeError)`。

根因：

- Android WebView 的 DOM Fullscreen 最终依赖 `WebChromeClient.onShowCustomView/onHideCustomView`；
- 当前生成的 `RustWebChromeClient.kt` 直接在 `onShowCustomView()` 里调用 `callback.onCustomViewHidden()`，等价于显式拒绝网页全屏；
- `src/scripts/html-code-preview.js` 创建的预览 `iframe` 未声明 fullscreen 权限，嵌入页面即使调用 `requestFullscreen()` 也缺少宿主授权。

原始问题定位：

- 新增 `AndroidWebFullscreenController.kt`，负责：
  - 将 WebView 请求的 custom view 挂到 Activity 内容根节点；
  - 全屏期间强制开启 immersive system bars，退出时恢复先前状态；
  - 暴露 `hide()`，让 Android 返回键优先退出网页全屏。
- `MainActivity.kt` 实现 `AndroidWebFullscreenHost`，只做生命周期编排与 controller 委托。
- `AndroidBackNavigationController.kt` 新增 native back 优先消费点，先尝试退出网页全屏，再决定是否把返回键交给前端/退出应用。
- `RustWebChromeClient.kt` 仅保留最小补丁：
  - `onShowCustomView()` 转发到 `AndroidWebFullscreenHost`
  - `onHideCustomView()` 转发到 `AndroidWebFullscreenHost`
- `src/scripts/html-code-preview.js` 为预览 `iframe` 增加 `allowfullscreen` / `allow="fullscreen"`。

### 8.1 进一步的架构收敛

上面的 fullscreen 逻辑本身没有问题，真正的问题是挂载位置：

- `RustWebChromeClient.kt` 来自 Wry Android 生成链；
- 直接改 `src-tauri/gen/android/.../generated/RustWebChromeClient.kt` 会在重新生成 Android 工程时被覆盖；
- `MainActivity.onWebViewCreate()` 又不是一个可靠的运行时替换点，因为 Wry 后续仍会再次调用 `setWebChromeClient(...)`。

因此，fullscreen 的正式方案不应继续依赖“修改 generated 文件”，而应改为：

- 在项目源码中提供本地 `com.tauritavern.client.RustWebChromeClient`；
- 在 Android Gradle 构建中排除 generated 版本参与编译；
- 让 Wry native 侧继续通过原有类名加载，但实际落到项目自维护实现。

### 8.2 正式维护原则

- 不再手改 `generated/RustWebChromeClient.kt`；
- local `RustWebChromeClient.kt` 只承担 Wry fullscreen 边界转发，不承载 fullscreen 状态机；
- fullscreen 业务逻辑必须继续留在自维护文件（`MainActivity.kt` / `AndroidWebFullscreenController.kt`），不要把状态机堆回 generated 文件；
- 不做前端 fullscreen polyfill 或静默降级，失败直接暴露，便于定位真实链路问题；
- 未来升级 Tauri / Wry 时，只需要对比 upstream 的 `RustWebChromeClient.kt` 与本地替代版本的差异。
