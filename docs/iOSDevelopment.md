# TauriTavern iOS 端开发说明

本文档记录当前 iOS 端开发中已经踩过的关键问题、根因分析、已落地方案，以及对应的架构改动。目标是避免重复踩坑，并确保移动端样式契约（`--tt-inset-*`）在 iOS 上可预测、可维护。

补充：iOS/iPadOS 的 **分发 Policy（profile + capabilities snapshot）** 属于“合规裁剪/能力分级”问题域，其当前实现快照与维护约束已收敛到 `docs/CurrentState/iOSPolicy.md`，本文件仍聚焦 WKWebView 行为差异与 iOS-only 桥接。

当前系统支持边界：

| iOS/iPadOS | `structuredClone` | Element Fullscreen | 支持级别 |
| --- | --- | --- | --- |
| 15.0–15.3 | 上游 vendored polyfill | 关闭 | 有限支持 |
| 15.4–15.x | WebKit 原生实现 | 关闭 | 有限支持 |
| 16.0–16.3 | WebKit 原生实现 | 开启 | 有限支持 |
| 16.4+ | WebKit 原生实现 | 开启 | 完整支持 |

`src/index.html` 已在 `init.js` 前加载 SillyTavern 1.18.0 的 `lib/structured-clone/monkey-patch.js`。它只在全局能力缺失时安装，15.4+ 不替换 WebKit 原生函数。

## 1. WKWebView safe-area 自动 inset 导致底部死区

### 1.1 现象

- 页面底部出现一块灰色、不可交互的区域。
- 前端根节点（如 `#sheld`）看似已撑满 `window.innerHeight`，但依然无法覆盖到屏幕底边。

### 1.2 关键定位信号

当出现如下特征时，优先判断为 **iOS native 侧对 WebView 做了 safe-area 自动 inset 调整**，而不是纯前端 CSS 高度问题：

- `screen.height - window.innerHeight` 显著大于 0（例如 `96px`）
- 同时 `env(safe-area-inset-bottom)`（或 `--tt-inset-bottom`）仍为非 0（例如 `34px`）

这通常意味着：**Web 内容的 viewport 被系统按 safe-area 扣掉了（顶部 + 底部）**，因此 DOM 只能布局在“安全区内的可视内容区域”，无法触达屏幕真实底边。

### 1.3 根因

WKWebView 内部是 `UIScrollView` 承载 Web 内容；在默认行为下，iOS 可能对该 scroll view 启用自动的内容 inset 调整（safe-area / scroll indicator insets），导致：

- Web 内容 viewport 变小（`window.innerHeight` 被扣减）
- 产生“看得见但不可交互”的底部空白区域（它不是 DOM 的一部分）

这会与当前的移动端布局契约冲突：iOS 侧 safe-area 应由前端通过 `env(safe-area-inset-*)` → `--tt-inset-*` 统一消费，而不是由 native 再额外“帮你扣一遍”。

### 1.4 已落地方案（fail-fast）

在 iOS 端创建主窗口后，对 WKWebView 的 `scrollView` 做一次性配置：

- `scrollView.contentInsetAdjustmentBehavior = .never`
- 清空 `contentInset` 与 `scrollIndicatorInsets`
- 关闭 `automaticallyAdjustsScrollIndicatorInsets`

该策略的目标是：让 `window.innerHeight` 覆盖到 full-bleed viewport；safe-area 的避让完全交给 CSS contract（`--tt-inset-*`）控制。

实现位置：

- iOS 配置入口：`src-tauri/crates/tauritavern/src/infrastructure/ios_webview.rs` 的 `configure_main_wkwebview()`
- 调用时机：`src-tauri/crates/tauritavern/src/app/host/window.rs`（主窗口 build 后立刻调用）

### 1.5 验收建议

修复后应满足：

- `screen.height - window.innerHeight` 接近 0（允许 1px 内的 rounding）
- `--tt-inset-bottom` 仍保持合理的 safe-area 值（如 `34px`），且输入框/按钮不被 home indicator 遮挡

## 2. data-migration：iOS 原生 Document Picker / Share Sheet 桥接

### 2.1 现象

- **导出**：UI 虽提示完成，但仅得到 iOS 沙盒内路径（对普通用户不可达），无法“拿到文件”。
- **导入**：能弹出文件选择器，但选择 zip 后无反馈/不启动导入。

### 2.2 根因（第一性原理）

iOS 上“文件选择 / 文件导出”必须交给系统级能力完成：

- WebView 无法向用户暴露可操作的沙盒路径（即使文件写入成功，用户也无法访问）。
- `<input type="file">` 在 WKWebView 上对 zip 的行为差异较大，不适合作为 data-migration 的唯一入口。
- 若宿主未安装 `WKUIDelegate` 的 JS dialog bridge，`window.alert/confirm/prompt` 可能不弹出或阻塞；当前已在 Host policy 层补齐（见 `docs/WkWebViewJsDialogBridgePlan.md`）。

### 2.3 已落地方案（当前状态：已稳定可用）

仅在 iOS 平台启用原生桥接：

1) **Import（Document Picker）**
   - 使用 `UIDocumentPickerViewController` 选择数据归档（当前后端支持 zip / tar / tar.gz / tgz；导出仍保持 zip）。
   - 将选中的 `file://` URL 复制到 app 内部 `archive_imports_root/incoming` staging，再启动现有 import job（job/轮询语义不变）。

2) **Export（Share Sheet）**
   - export job 生成 zip 后，不再尝试“保存到 Downloads 并展示路径”。
   - 直接使用 `UIActivityViewController` 打开 Share Sheet，让用户保存到 Files / AirDrop / 其它 App。

3) **UI 线程与呈现约束**
   - 所有 UIKit present 均通过 `WebviewWindow::run_on_main_thread` 执行，并通过 `UIApplication.windows` 解析 top-most presenting VC。
   - iPad 走 popoverPresentationController 绑定 sourceView/sourceRect，避免崩溃。

4) **确认弹窗**
   - iOS 导入确认使用 `Popup.show.confirm`（保持与 SillyTavern 交互契约一致；不依赖同步阻塞式 dialog）。
   - 其他平台保持原语义不变。

### 2.4 重要实现位置（便于维护与回归）

- 前端扩展入口：`src/scripts/extensions/data-migration/index.js`
- Host Kernel 路由：`src/tauri/main/routes/extensions-routes.js`
- iOS-only Tauri commands：`src-tauri/crates/tauritavern/src/presentation/commands/ios_file_bridge_commands.rs`
- iOS UIKit / picker / share host adapter：
  - `src-tauri/crates/tauritavern/src/platform/ios_ui.rs`
  - `src-tauri/crates/tauritavern/src/platform/ios_document_picker.rs`
  - `src-tauri/crates/tauritavern/src/platform/ios_share_sheet.rs`
- Data Archive import staging 只暴露 `prepare_data_archive_import_target_path`；不要恢复旧的 `get_data_archive_imports_root`，避免把 staging root 交给前端自行拼路径。
- 角色卡替换的文件选择同样走 iOS Document Picker + app cache/temp staging；宿主只返回 staged path/原文件名，前端会读成标准 `File` 后继续复用上游 `/api/characters/import` 与 `preserved_name` 语义。当前 native picker 只暴露 Rust 角色导入器真实支持的 `json/png`。

### 2.5 macOS 元数据导致的“布局歧义”问题

部分 zip（尤其是从 macOS Finder 打包/转发）会携带 `__MACOSX/**` 资源分叉条目；它会在布局探测阶段制造“存在多个候选根”的假象，触发错误：

- `Invalid data: Archive layout is ambiguous`

当前实现会在 **布局扫描** 与 **解压归一化** 时忽略 `__MACOSX` 条目，保证这类 zip 可正常导入：

- `src-tauri/crates/tt-adapter-archive/src/data_archive/import/layout.rs`
- `src-tauri/crates/tt-adapter-archive/src/data_archive/import/extract.rs`

## 3. 通用 iOS 导出桥（聊天 / WorldInfo / 角色卡等）

### 3.1 现象

- 聊天导出、WorldInfo 导出、角色卡导出等“浏览器式下载”在 iOS 上可能无响应，或只写进沙盒内不可达路径。
- 同源导出端点即使返回了正确的二进制，WKWebView 的默认下载语义也不能保证用户真正拿到文件。

### 3.2 根因（第一性原理）

- iOS 上真正可交付给用户的文件出口是系统 Share Sheet，而不是 WebView 默认下载目录。
- 上游 SillyTavern 的导出语义是“下载一个文件”，不是“调用某个 iOS 业务 API”；因此平台差异必须集中在导出基础设施层吸收。
- 用全局 monkey-patch `HTMLAnchorElement.prototype.click()` 虽然能扩大覆盖面，但会把浏览器基本语义变成宿主隐式契约，长期不利于维护、调试与兼容升级。

### 3.3 当前契约（已落地）

1. 前端统一导出主链仍是 `download()` / `downloadBlobWithRuntime()`。
2. iOS 分支会把 `Blob` staging 到临时目录后调用 `ios_share_file`，再弹出 Share Sheet。
3. `download-bridge.js` 只负责同源窗口中的浏览器式下载桥接：
   - 支持 `blob:` / `data:` / 同源 `http(s)` 或相对 URL；
   - 仅接管带 `download` 属性的 anchor；
   - 只保留 document capture 监听，不再 monkey-patch `HTMLAnchorElement.prototype.click()`。
4. Rust 命令 `ios_share_file` 只接受 app `tempDir` / `appCacheDir` 下专用 staging root `tauritavern-export-staging` 内的绝对路径，避免前端获得“任意本地文件分享”能力。

### 3.4 失败与清理语义

- 分享弹窗展示失败、文件不存在、路径越界等错误必须直接失败并向用户可见。
- 用户主动取消 Share Sheet 不算错误，返回 `completed: false`，不显示成功 toast。
- staging 清理属于 best-effort：
  - 如果分享阶段已经结束，cleanup 失败只记录告警，不反向污染分享结果；
  - 如果 staging 尚未完成就失败，前端会立即尝试回收临时目录，避免残留堆积。

### 3.5 维护约束

- 业务代码如果需要程序化导出，优先走共享 `download()` / `downloadBlobWithRuntime()`，不要重新发明 iOS 特判。
- 如果未来需要扩大 `ios_share_file` 能力边界，应先重新设计 staging contract，而不是放宽到任意沙盒路径。
- 如果未来出现大文件同源下载需求，应优先考虑 stream-to-file，而不是继续 `fetch -> blob -> share` 扩容。

实现位置：

- 前端导出基础设施：`src/scripts/file-export.js`
- 下载桥：`src/tauri/main/download-bridge.js`
- 导出反馈：`src/scripts/download-feedback.js`
- iOS share 命令：`src-tauri/crates/tauritavern/src/presentation/commands/ios_file_bridge_commands.rs`

### 3.6 iOS Skill 导入

Skill 导入使用独立命令 `ios_pick_skill_import_archives`：

- `UIDocumentPickerViewController` 允许选择一个或多个 zip / 普通 data 文件，保证默认 `.zip` Skill 归档与历史 `.ttskill` 归档都可被选中；
- Rust 命令逐个把选中的安全作用域文件复制到 app cache/temp 下的 `tauritavern-skill-import-staging`；任一复制失败时清理本次命令已经 staged 的文件并返回错误；
- 前端仍只收到一个或多个 `{ kind: 'archiveFile', path }`，后续预览与安装继续逐项走 Skill repository 的真实路径契约；
- 用户放弃某个输入时由 `api.skill.discardPickedImport(input)` 清理；放弃整个批次时调用无参数的 `discardPickedImport()`。安装完成或失败后由 `installImport()` 自动清理对应输入。

## 4. WKWebView Element Fullscreen（iOS 16+ 启用）

### 4.1 现象

- 角色卡或扩展内的同源 iframe 页面在桌面/Android 可进入全屏，但 iOS 上 `requestFullscreen()` / `webkitRequestFullscreen()` 不生效。

### 4.2 根因

- 问题不在前端按钮或 JS-Slash-Runner 事件语义，而在宿主 WKWebView 默认没有开启 element fullscreen 能力。
- TauriTavern 的兼容目标仍然是让上游页面继续使用标准浏览器 Fullscreen API，而不是引入额外 JS-native bridge。

### 4.3 已落地方案

- 继续复用 `src-tauri/crates/tauritavern/src/infrastructure/ios_webview.rs` 的主 WebView 配置入口，在 `configure_main_wkwebview()` 内统一完成两类 native 配置：
  - 关闭 `scrollView` 的 safe-area 自动 inset 调整；
  - 仅在 iOS 16.0+ 开启 `WKPreferences.setElementFullscreenEnabled(true)`。
- 这样角色卡、JS-Slash-Runner、同源 iframe 的 fullscreen 事件、退出语义和上游契约保持一致，宿主只补齐平台能力，不改前端行为。

### 4.4 支持边界

- 公开 `WKPreferences.elementFullscreenEnabled` 从 iOS 15.4 可用且默认关闭；TauriTavern 仍将 iOS 16.0 作为产品 fullscreen feature floor。
- iOS 15 保持 WebKit 默认关闭，不删除 iframe 权限、不改写 `requestFullscreen()`，让页面继续观察标准浏览器失败语义。
- iOS 16.0–16.3 可使用 fullscreen，但仍属于有限支持；完整支持从 iOS 16.4 开始。

### 4.5 构建版本契约

- 最低部署版本是 `15.0`，规范值写在 `tauri.conf.json` 的 `bundle.iOS.minimumSystemVersion`。
- `gen/apple/project.yml`、`Podfile` 与已提交 `.xcodeproj` 必须保持同值；Xcode pre-build script 会将真实 `IPHONEOS_DEPLOYMENT_TARGET` 与 Tauri 配置比较，不一致时直接失败。
- 当前 `project.yml` 未覆盖已提交 Apple host 的全部自定义 Info.plist、scheme 与签名状态。不要直接运行 XcodeGen 或 `tauri ios init` 覆盖工程；修改后只审查必要 diff，否则可能删除高刷、后台模式与文件类型声明。

## 5. iOS 分发 Policy（当前状态）

iOS 外测/内测分发裁剪与能力分级已落地为 iOS-only 的 `ios_policy` 运行时系统（可被导入 `tauritavern-settings.json` 覆盖 profile/能力边界，且 iOS 上 fail-fast、桌面端忽略）。

- 当前实现快照：`docs/CurrentState/iOSPolicy.md`

## 6. iOS 18+ App Icon 外观变体

### 6.1 现象

iOS 深色图标模式下，App 放入文件夹后可能出现“文件夹外仍是普通图标，打开文件夹后变成深色图标”的不一致表现。

### 6.2 根因

iOS 18+ 支持 Home Screen 图标的 `Any` / `Dark` / `Tinted` 外观。若 AppIcon 只提供传统多尺寸 `Any` 图标，系统会自动生成深色/着色效果；文件夹缩略图和展开文件夹可能走不同缓存或渲染路径，从而出现外观不一致。

### 6.3 当前方案

`AppIcon.appiconset` 改为 Xcode single-size 1024px 源图，并显式提供：

- `AppIcon-Light.png`：基础 `Any` 图标，不透明背景。
- `AppIcon-Dark.png`：深色图标，透明背景，交给系统深色底承载。
- `AppIcon-Tinted.png`：着色图标，透明背景，灰度前景。

维护入口：

- 生成脚本：`scripts/generate-ios-app-icon-variants.swift`
- 构建期校验/展平：`scripts/ios-opaque-app-icons.swift`
- 资产目录：`src-tauri/crates/tauritavern/gen/apple/Assets.xcassets/AppIcon.appiconset`

重新生成：

```sh
xcrun --sdk macosx swift scripts/generate-ios-app-icon-variants.swift \
  src-tauri/crates/tauritavern/icons/icon.png \
  src-tauri/crates/tauritavern/gen/apple/Assets.xcassets/AppIcon.appiconset
```

回归验证：

```sh
xcrun actool --compile /tmp/tt-appicon \
  --platform iphonesimulator \
  --minimum-deployment-target 15.0 \
  --app-icon AppIcon \
  --output-partial-info-plist /tmp/tt-appicon/partial.plist \
  src-tauri/crates/tauritavern/gen/apple/Assets.xcassets

xcrun assetutil --info /tmp/tt-appicon/Assets.car
```

输出应包含 `UIAppearanceDark` 与 `ISAppearanceTintable`。若未来重新运行 `tauri icon`，必须重新生成并保留这三个 appearance 变体。

## 7. AI 生成后台执行

iOS Chat Completion 的后台租约由 Rust `ChatCompletionService` 持有，不依赖 WKWebView 的 Promise、Channel 或定时器继续运行：

- iOS 26+ 的用户可见生成使用 `BGContinuedProcessingTask`，任务由用户操作立即提交，并在系统 Live Activity 中展示；
- 流式生成以累计收到的 provider 响应字节作为单调递增的真实进度。因为 LLM 输出总量事先未知，`NSProgress` 保持 indeterminate，成功结束时再收敛到 100%；
- iOS 16–25，以及不会展示系统 UI 的 quiet 内部生成，继续使用 `beginBackgroundTask`。`BGContinuedProcessingTask` 不适合无明确用户意图却会显示 Live Activity 的后台工作。

生命周期契约：

- 普通与流式 Chat Completion 在网络请求前取得后台租约；
- 成功、失败或用户取消都会由 Rust 立即完成对应的系统任务；
- 若系统先触发 expiration handler，原生适配器只结束后台保护并记录警告，不把“后台保护结束”伪装成“生成失败”；
- 系统随后可以挂起应用；若进程仍存活，生成在恢复执行后继续等待真实 provider 结果、网络错误或用户取消；
- 已产生的流事件仍由 Rust 进程内会话保留，WebView 恢复后通过 `after_seq` 继续读取；
- 无法取得后台租约只记录明确警告，不阻塞仍在前台可正常完成的生成。

当前不使用 `BGAppRefreshTask` / `BGProcessingTask`，因为它们由系统择机调度，不能表达用户点击后立即开始的聊天生成。`BGContinuedProcessingTask` 只拥有系统后台执行权和展示状态；真正的网络请求、取消和流会话生命周期仍由既有 Rust 服务负责，不引入第二套生成调度状态机。
