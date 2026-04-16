# TauriTavern iOS 端开发说明

本文档记录当前 iOS 端开发中已经踩过的关键问题、根因分析、已落地方案，以及对应的架构改动。目标是避免重复踩坑，并确保移动端样式契约（`--tt-inset-*`）在 iOS 上可预测、可维护。

补充：iOS/iPadOS 的 **分发 Policy（profile + capabilities snapshot）** 属于“合规裁剪/能力分级”问题域，其当前实现快照与维护约束已收敛到 `docs/CurrentState/iOSPolicy.md`，本文件仍聚焦 WKWebView 行为差异与 iOS-only 桥接。

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

- iOS 配置入口：`src-tauri/src/infrastructure/ios_webview.rs` 的 `configure_main_wkwebview()`
- 调用时机：`src-tauri/src/lib.rs`（主窗口 build 后立刻调用）

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
- `window.confirm()` 在 iOS WebView 上存在不可靠性（可能不弹出/阻塞），会导致“看起来无反应”。

### 2.3 已落地方案（当前状态：已稳定可用）

仅在 iOS 平台启用原生桥接：

1) **Import（Document Picker）**
   - 使用 `UIDocumentPickerViewController` 选择 `.zip`。
   - 将选中的 `file://` URL 复制到 app 内部 `archive_imports_root/incoming` staging，再启动现有 import job（job/轮询语义不变）。

2) **Export（Share Sheet）**
   - export job 生成 zip 后，不再尝试“保存到 Downloads 并展示路径”。
   - 直接使用 `UIActivityViewController` 打开 Share Sheet，让用户保存到 Files / AirDrop / 其它 App。

3) **UI 线程与呈现约束**
   - 所有 UIKit present 均通过 `WebviewWindow::run_on_main_thread` 执行，并通过 `UIApplication.windows` 解析 top-most presenting VC。
   - iPad 走 popoverPresentationController 绑定 sourceView/sourceRect，避免崩溃。

4) **确认弹窗**
   - iOS 导入确认使用 `Popup.show.confirm`（避免 `window.confirm` 在 iOS 上不可靠）。
   - 其他平台保持原语义不变。

### 2.4 重要实现位置（便于维护与回归）

- 前端扩展入口：`src/scripts/extensions/data-migration/index.js`
- Host Kernel 路由：`src/tauri/main/routes/extensions-routes.js`
- iOS-only Tauri commands：`src-tauri/src/presentation/commands/ios_file_bridge_commands.rs`
- iOS UIKit 封装：
  - `src-tauri/src/infrastructure/ios_ui.rs`
- `src-tauri/src/infrastructure/ios_document_picker.rs`
- `src-tauri/src/infrastructure/ios_share_sheet.rs`

### 2.5 macOS 元数据导致的“布局歧义”问题

部分 zip（尤其是从 macOS Finder 打包/转发）会携带 `__MACOSX/**` 资源分叉条目；它会在布局探测阶段制造“存在多个候选根”的假象，触发错误：

- `Invalid data: Archive layout is ambiguous`

当前实现会在 **布局扫描** 与 **解压归一化** 两阶段一致忽略 `__MACOSX` 条目，保证这类 zip 可正常导入：

- `src-tauri/src/infrastructure/persistence/data_archive/import/layout.rs`
- `src-tauri/src/infrastructure/persistence/data_archive/import/extract.rs`

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
- iOS share 命令：`src-tauri/src/presentation/commands/ios_file_bridge_commands.rs`

## 4. WKWebView Fullscreen API（iOS 16+）

### 4.1 现象

- 角色卡或扩展内的同源 iframe 页面在桌面/Android 可进入全屏，但 iOS 上 `requestFullscreen()` / `webkitRequestFullscreen()` 不生效。

### 4.2 根因

- 问题不在前端按钮或 JS-Slash-Runner 事件语义，而在宿主 WKWebView 默认没有开启 element fullscreen 能力。
- TauriTavern 的兼容目标仍然是让上游页面继续使用标准浏览器 Fullscreen API，而不是引入额外 JS-native bridge。

### 4.3 已落地方案

- 继续复用 `src-tauri/src/infrastructure/ios_webview.rs` 的主 WebView 配置入口，在 `configure_main_wkwebview()` 内统一完成两类 native 配置：
  - 关闭 `scrollView` 的 safe-area 自动 inset 调整；
  - 开启 `WKPreferences.setElementFullscreenEnabled(true)`。
- 这样角色卡、JS-Slash-Runner、同源 iframe 的 fullscreen 事件、退出语义和上游契约保持一致，宿主只补齐平台能力，不改前端行为。

### 4.4 支持边界

- 该能力依赖 iOS 16 的 app-embedded WKWebView Fullscreen API，因此项目 iOS 支持线提升到 `iOS 16+`。

## 5. iOS 分发 Policy（当前状态）

iOS 外测/内测分发裁剪与能力分级已落地为 iOS-only 的 `ios_policy` 运行时系统（可被导入 `tauritavern-settings.json` 覆盖 profile/能力边界，且 iOS 上 fail-fast、桌面端忽略）。

- 当前实现快照：`docs/CurrentState/iOSPolicy.md`
