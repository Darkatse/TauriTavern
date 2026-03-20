# TauriTavern iOS 端开发说明

本文档记录当前 iOS 端开发中已经踩过的关键问题、根因分析、已落地方案，以及对应的架构改动。目标是避免重复踩坑，并确保移动端样式契约（`--tt-inset-*`）在 iOS 上可预测、可维护。

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

- iOS 配置入口：`src-tauri/src/infrastructure/ios_webview.rs`
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
