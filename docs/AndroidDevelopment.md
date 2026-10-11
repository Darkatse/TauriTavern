# TauriTavern Android 端开发说明

本文档记录当前移 Android 端开发中已经踩过的关键问题、根因分析、已落地方案，以及对应的架构改动。目标是避免重复踩坑，并为后续替换官方修复留出清晰迁移路径。

当前支持 Android 8.0（API 26）及以上，并要求系统 WebView/Chrome 已更新到能执行 ES2020 的版本。

## 构建工具链

Gradle 工程跟随 Tauri CLI 的 Android 模板，当前对齐 2.12.3：Gradle 9.6.1、AGP 9.3.1、Kotlin Gradle Plugin 2.2.10。升级 Tauri 时按模板差异同步；AGP 版本同时写在根 `build.gradle.kts` 和 `buildSrc/build.gradle.kts`，两处一起改。

- `compileSdk`、`targetSdk` 为 36。targetSdk 37 起访问局域网需要 `ACCESS_LOCAL_NETWORK` 运行时权限，提升前先建立统一的本地网络权限入口。
- 开启 configuration cache：`BuildTask` 经 `ExecOperations` 调用 CLI，执行阶段不访问 `Project`。
- `android.builtInKotlin=false`、`android.newDsl=false` 与模板一致，因为 Tauri 与官方插件仍应用 `org.jetbrains.kotlin.android`。
- release 沿用 `proguard-android-optimize.txt`，`@JavascriptInterface` 方法由其中的默认规则保留；`proguard-rules.pro` 只补依赖缺失的规则，原因写在规则旁。

## 1. 原生内容视口与窗口背景

`AndroidWindowLayout` 拥有主 WebView 的矩形：系统栏、刘海和停靠键盘在原生层消费，网页按普通视口排版。`WindowLayoutPlugin` 供 Rust 读取窗口快照、提交背景条带。职责、提交时机与背景链路见 [CurrentState/MobileStyleAdaptation.md](CurrentState/MobileStyleAdaptation.md)。

清单中的 `adjustResize` 只供 androidx 在 API 26-29 上推导键盘 inset；布局不依赖系统缩放窗口。

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

`src-tauri/crates/tauritavern/build.rs` 现在会：

- 扫描 `../default/content`，生成 `default_content_manifest.json`（默认内容清单）；
- 需要内嵌资源时，把前端模板与内置扩展的 HTML 模板写入 `embedded_resources.rs`（虚拟路径 -> `include_bytes!` 映射）。

#### B. 运行时统一资源访问入口

`src-tauri/crates/tauritavern/src/infrastructure/assets.rs` 提供统一 API：

- `read_resource_bytes`
- `read_resource_text`
- `read_resource_json`
- `copy_resource_to_file`
- `list_default_content_files_under`

平台策略：

- Android 与 OpenHarmony：`build.rs` 发出 `embedded_resources`，只读嵌入映射；
- 其他平台：走 `BaseDirectory::Resource` + fs 访问；portable 构建在磁盘未命中时回退到嵌入映射。

#### C. 前后端模板读取解耦

- 后端新增命令：`read_frontend_template`  
  文件：`src-tauri/crates/tauritavern/src/presentation/commands/bridge.rs`
- 前端模板加载改为 Tauri 环境下优先 invoke：  
  文件：`src/scripts/templates.js`
- 内置扩展模板只在嵌入映射中，桌面 bundle 不含，所以 `templates.js` 仅在 Android 上用 `read_frontend_extension_template` 读取，其他宿主走 fetch。

#### D. 默认内容初始化改为“资源 -> 真实文件”复制流程

`src-tauri/crates/tauritavern/src/infrastructure/repositories/file_content_repository.rs` 不再依赖资源目录的直接文件路径语义，改用统一资源接口复制到用户目录。

---

## 3. iOS / Android 应用数据目录解析异常

### 3.1 问题背景

在移动端，Tauri 提供的目录 API 在不同平台/版本可能与预期目录不一致。  
已确认 Android 存在已知问题：`appDataDir/localDataDir` 可能返回内部路径（如 `/data/user/0/...`）而非外部 app 目录（如 `/storage/emulated/0/Android/data/...`）。

### 3.2 当前方案：单点路径解析抽象

新增单点路径解析模块：  
`src-tauri/crates/tauritavern/src/infrastructure/paths.rs`

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

- `src-tauri/crates/tauritavern/src/app.rs` 的 `resolve_data_root` / `resolve_log_root` 已改为依赖 `resolve_app_data_dir`

### 4.3 资源协议访问权限

- `src-tauri/crates/tauritavern/src/lib.rs` 在 setup 阶段对 `data_root` 执行：
  - `asset_protocol_scope().allow_directory(&data_root, true)`
- 目的：允许 WebView 通过 asset 协议访问用户数据文件，避免前端资源加载 403。

### 4.4 前端接入点

- `src/scripts/templates.js`：模板读取在 Tauri 环境下走 `invoke('read_frontend_template')`
- `src/css/mobile-styles.css` + `src/style.css`：按已经避让的浏览器视口排版，`--doc-height` 为静态 CSS 别名

---

## 5. 后续迁移与清理建议

1. **Tauri 官方修复目录 API 后**  
   `infrastructure/paths.rs` 会自动优先使用修复后的 `app_data_dir`，无需在仓储层做分散修补。

2. **新增移动端特性时**
   优先复用现有单点抽象（`assets.rs` / `paths.rs` / `MainActivity.kt`），避免再次把平台差异扩散到业务代码。

---

## 6. AI 生成后台执行与通知生命周期

Android AI 生成使用任务级 `dataSync` Foreground Service。Rust `ChatCompletionService` 持有生成任务与后台执行租约；WebView 只负责消费流事件，并在 Android 16+ 可用时补充非关键的 token 进度。

执行契约：

- 第一个 Chat Completion 任务开始时，由 Rust 通过原生 Tauri plugin 启动 `AiGenerationForegroundService`；
- 并发任务通过稳定 task id 计数，共享一个 FGS；
- 最后一个任务成功、失败或取消后立即 `stopForeground()` + `stopSelfResult()`；
- Service 使用 `START_NOT_STICKY`，不会在没有真实生成任务时被系统复活；
- Android 15+ `dataSync` 超时时通过 `onTimeout()` 立即释放 FGS，但不把平台保护到期升级为生成失败；
- 应用启动本身不再启动保活服务，避免无任务时消耗 Android 的后台 FGS 配额。
- 原生插件不可用或 FGS 启停失败只记录警告；Chat Completion 仍按真实 provider 结果继续，不阻塞应用启动或生成。

当前通知槽位：

- `42000`：前台服务保活通知，只维持 Android 后台执行契约；
- `42001`：生成完成通知，表示需要用户知晓的一次完成/失败结果。

生命周期契约：

- 应用前台可交互态定义为 `Activity resumed && window focused`；
- 应用进入前台可交互态、冷启动、或收到新的 launch intent 时，只清除 `42001`；
- Rust 任务结束时，如果应用已经前台可交互、请求为 quiet 或任务被取消，则不发布完成通知；
- 发布新的完成通知前先清除旧的 `42001`，避免 fixed notification id 上的静默复用；
- 完成通知不使用 `onlyAlertOnce`；保活/进度通知仍可使用，避免 token 进度频繁打扰。

维护原则：

- 不要用 `cancelAll()` 清通知，避免误伤系统或未来扩展通知；
- 不要把 native completion 能力绑定到 Android 16+ `ProgressStyle`，旧版 Android 也需要完成通知生命周期；
- 不要把 FGS 的结束重新绑定到 WebView 回调；WebView 被系统挂起时，Rust 任务仍必须能够独立释放原生租约；
- 不要让前端承担 Android 通知栏清理职责，前端应继续保持上游 SillyTavern 的事件语义。

---

## 7. 插件系统（前端）移动端兼容补丁

以下问题仅在 Android 旧 WebView 上高概率出现，桌面端通常不复现。

### 7.1 JavaScript 运行时兼容

#### `*.at is not a function`

现象：

- 第三方插件初始化报错（典型如 `g.at is not a function`）。

根因：

- 插件构建产物使用了较新的 JS API（`Array/String.at`、`toSorted`、`findLastIndex` 等）。
- 旧 Android WebView 缺少这些 API。

已落地方案：

- 在 Tauri mobile 启动期安装运行时兼容层：
  - 实现：`src/tauri/main/compat/mobile/mobile-runtime-compat.js`
  - 入口：`src/tauri/main/bootstrap.js`（仅移动宿主）
  - 行为：基础 API 仅在缺失时补齐，且只执行一次；桌面宿主不启用。

#### Web Clipboard 写入被拒绝

Android WebView 可能暴露 `navigator.clipboard.writeText()`，却在调用时以 `NotAllowedError` 拒绝写入。TauriTavern 第一方复制在所有平台统一走 `writeClipboardText()`；Android 兼容层只把上游 Web Clipboard 的 `writeText` 映射到同一原生写入器，并保留 Clipboard 对象上的其他方法。

原生侧只授予 `clipboard-manager:allow-write-text`。写入剪贴板不需要 Android manifest 或运行时权限，也不开放读取能力；失败直接返回调用方，不静默回退。

### 7.2 插件面板样式大面积失效（如 `TH-custom-tailwind` 布局错乱）

现象：

- 插件 CSS 文件请求成功，但大量样式未生效，界面排布混乱。

根因：

- 旧 Android WebView 对 CSS Cascade Layers（`@layer`）支持不完整。
- 采用 Tailwind v4 打包的插件会把大量规则放在 `@layer` 中，导致整层失效。

已落地方案：

- 在 `src/scripts/extensions/runtime/third-party-runtime.js` 的样式加载链路中：
  - 先探测当前 WebView 是否支持 `@layer`；
  - 不支持时为样式 URL 附加 `ttCompat=layer`；由 Rust 端点返回展平后的 CSS bytes。

性能策略：

- 支持 `@layer` 的环境走快路径，不改写 URL；
- 不再在前端预取/Blob 注入，避免低端设备 CSS AST 处理导致的卡顿与超时。


### 6.3 第三方面板可达性

脚本、portal、iframe 与 `<dialog>` 的 `top: 0` 就是内容视口顶部，`100vh` 随停靠键盘收缩。只有某个面板越界时，检查它自己的硬编码尺寸、负坐标或滚动裁剪；所有内容一起偏移时，检查原生 WebView 矩形。

---

## 7. Android 返回键分层返回（Back Navigation）

问题：

- Android 端按系统返回键可能直接退出应用（未能按“退一层 UI”关闭弹窗/抽屉/聊天）。
- 不要依赖覆盖 `Activity.onBackPressed()`：在较新的 Android（predictive back / `OnBackInvokedDispatcher`）路径下，Back 分发优先走 `OnBackPressedDispatcher`，`super.onBackPressed()` 也可能绕开子类 override。

当前方案（Native→JS Back Bridge）：

- `MainActivity` 在 `onCreate()` 里向 `onBackPressedDispatcher` 注册回调，并把 Back 交给 `AndroidBackNavigationController`：
  - `src-tauri/crates/tauritavern/gen/android/app/src/main/java/com/tauritavern/client/MainActivity.kt`
  - `src-tauri/crates/tauritavern/gen/android/app/src/main/java/com/tauritavern/client/AndroidBackNavigationController.kt`
- controller 通过 `WebView.evaluateJavascript` 调用前端全局函数：`window.__TAURITAVERN_HANDLE_BACK__()`。
  - JS 返回 `true`：表示已消费 Back（关闭了一层 UI），原生不退出。
  - JS 返回 `false`：表示前端未消费，原生执行 `finish()` 退出。

前端分层关闭策略：

- Back 逻辑集中在 `src/tauri/main/back-navigation.js`，并在 `src/tauri/main/bootstrap.js` 启动早期安装。
- 关闭动作必须复用现有 UI 的关闭入口（点击 close/cancel 或触发既有的“点空白收起”逻辑），避免引入新的状态机。
- “点空白收起”要模拟 `mousedown`（SillyTavern 绑定在 `html` 的 `touchstart/mousedown` 上），仅派发 `click` 不足以关闭抽屉。

维护原则：

- `src-tauri/crates/tauritavern/gen/android/.../generated/*` 是由 Cargo.lock 锁定的 Tauri/Wry 构建脚本重建的派生物，不纳入版本控制，也不承载本地语义。
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
  - 全屏期间隐藏系统栏；这是独立的展示状态，不改变用户沉浸偏好与主 WebView 矩形；
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
- 直接改 `src-tauri/crates/tauritavern/gen/android/.../generated/RustWebChromeClient.kt` 会在重新生成 Android 工程时被覆盖；
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

### 8.3 Wry Android 生成层所有权

`generated/*` 只是一份可重建的构建输出。项目实际维护的 Wry 分叉只有 generated 目录外、同 package 同类名的两个文件，Gradle 排除对应 generated 类以避免重复编译：

- `RustWebChromeClient.kt`：fullscreen 转发与 JSONL MIME 补充；Activity result 与权限 launcher 归上游 `WryActivity`，本类只调用其接口；
- `RustWebViewClient.kt`：拦截失败日志与错误响应，以及 Host Resource 显式缓存策略优先级。

两个文件头必须记录当前 Wry baseline。升级 Wry 时逐文件与锁文件解析到的 upstream 模板比较；缺少显式 `Cache-Control` 的自定义协议响应采用 Wry 的 `no-store` 默认值，Host Resource 已明确返回的 `private, no-cache` 或错误 `no-store` 不得被 transport 层覆盖。删除 generated 目录后，debug 与 minified release 构建都必须能够从零重建。

`app/tauri.build.gradle.kts` 同样是 ignored 派生物：`tauri-build` 会在其中声明 `androidx.lifecycle:lifecycle-process`。tracked `app/build.gradle.kts` 只应用该脚本，不重复维护依赖版本；从空生成目录完成 canonical debug/release 构建用于证明生成顺序和依赖闭合。

---

## 9. Android WebView 视频背景 Range 语义差异（SillyTavern-VideoBackgrounds）

现象：

- Android 端 `<video src="/backgrounds/*.mp4">` 无法进入播放。默认 poster 是透明的，不能凭播放按钮判断是否已加载。
- DevTools 常见表现为：
  - 早期出现 `416 Range Not Satisfiable` 或某个 `Range: bytes=...-` 请求被快速 canceled；
  - `<video>` 长时间停留在 `readyState=HAVE_NOTHING`，无法触发 `loadedmetadata`。

根因（运行时语义差异）：

- Android WebView 在 `shouldInterceptRequest` 的资源拦截链路中，会对“拦截返回的响应流”再次应用请求 Range 语义。
- 若宿主已经按 Range 做了 seek/slice（返回已经截取过的 bytes），WebView 的二次 Range 会把非 0 起点范围再次应用到截取后的流上，导致不可满足（历史上表现为 `416` / canceled）。

当前已落地 workaround（仅背景视频）：

- 实现：`src-tauri/crates/tt-application/src/services/host_resource_service/user_data.rs`
- 策略：对 Android + `/backgrounds/*` + `video/*` + `Range start != 0`：
  - 返回 `206` + 正确 `Content-Range/Content-Length`
  - body 提供完整文件 bytes，让 WebView 自己在流上执行 Range（skip）

更多细节与全平台媒体契约见：`docs/CurrentState/MediaAssetContract.md`。

---

## 10. Android 文件导入与导出

Android 是[文件传输](CurrentState/FileTransfer.md)的一个平台实现，与鸿蒙共用 URI 复制代码。`ContentUriPlugin` 负责查询原文件名。升级 Tauri 或 dialog/fs 插件后，要用 release 构建验收两点：Rust 调用原生插件是否正常；从云盘 provider 选取时，文件名与内容是否正确。

图片长按保存：Android WebView 没有图片上下文菜单，由 `download-bridge.js` 在页面层补上 `<img>` 的 `contextmenu` 默认行为，契约见 [FrontendHostContract §5.7](FrontendHostContract.md)。之所以放在页面层，是因为原生长按菜单会先消费长按，页面和扩展就收不到 `contextmenu`。Chromium 将来如果默认提供图片菜单，这一补充即可删除。

## 11. Android 大型 byte ingress

Tauri Android 当前不支持 raw byte invoke；嵌套 `Uint8Array` 会被编码为数字数组，放大内存占用。

聊天与世界书使用[共享提交会话](CurrentState/ChatPayload.md#3-统一聊天提交)：

- 按 host 帧上限仅编码当前帧的 base64，收到 ACK 后再发送下一帧；失败不回退 raw 传输。
- 前端只持有会话 ID，暂存与目标路径由 Rust 管理。
- 普通文件上传与 Blob 导出使用 `stage_file_*`，不承担文档发布。

升级 Tauri 后需在真机验证传输支持，不能仅依据 API 类型判断。

## 12. LAN Sync 多播发现

Android 的 Rust mDNS 发现需要 `CHANGE_WIFI_MULTICAST_STATE` 权限。宿主只在 Activity 处于前台时持有 `MulticastLock`：停止发现或插件收到 `onPause(activity)` 时释放，Activity resume 后由 Rust 为已启动的发现重新获取；网络逻辑由 Rust 负责。插件构造参数保持 `Activity`，与 Tauri 的 JNI 加载签名一致。

平台适配见 [LanDiscoveryPlugin](../src-tauri/crates/tauritavern/gen/android/app/src/main/java/com/tauritavern/client/LanDiscoveryPlugin.kt)，功能边界见[同步总览](CurrentState/Sync.md)。

## 13. Tauri Pilot

`pnpm run android:dev:pilot` 构建带 Pilot 的 debug 包。插件在应用内监听 abstract socket，名字随每次启动变化，CLI 无法自动发现；页面加载后转发到私有目录，并用 `TAURI_PILOT_SOCKET` 指给 CLI：

```bash
name=$(adb shell cat /proc/net/unix | grep -o 'tauri-pilot-com\.tauritavern\.-[0-9a-f]*\.sock' | tail -1)
dir=$(mktemp -d /tmp/tauri-pilot.XXXXXX)
adb forward "localfilesystem:$dir/pilot.sock" "localabstract:$name"
export TAURI_PILOT_SOCKET="$dir/pilot.sock"
tauri-pilot ping
```

应用重启后旧转发失效，重新执行以上命令；结束时 `adb forward --remove "localfilesystem:$dir/pilot.sock"` 并删除该目录。多台设备时给每条 `adb` 命令加 `-s <serial>`。Android 上没有 `press`，文字输入用 `fill` 或 `type`。
