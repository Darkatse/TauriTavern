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

