# 移动端样式适配现状（Edge‑to‑Edge / Safe‑Area / 沉浸模式）

本文档描述 **已经落地** 的移动端（Android / iOS）样式与布局适配现状，重点覆盖：

- Edge‑to‑edge（透明系统栏、刘海区域扩展）
- Safe‑area / IME inset 的注入与消费（CSS 变量契约）
- 沉浸模式（隐藏 system bars）下仍避开 display cutout（刘海/打孔）
- 第三方脚本注入浮层的 safe‑area top 兜底（元素级补丁）

## 1. 范围与结论

结论（当前实现的核心要点）：

1. **Safe‑area 是“宿主提供的几何事实”**：Android 由 native 监听 `WindowInsets` 注入 `--tt-safe-area-*`；iOS 以 CSS `env(safe-area-inset-*)` 为主。
2. **沉浸模式不等于安全区为 0**：Android 沉浸（system bars 隐藏）时，`--tt-safe-area-*` 仍保留 `displayCutout` inset，确保刘海机型第三方浮层不会沉入刘海/状态栏区域。
3. **第三方浮层只做元素级最小修正**：不重写 `<style>` 文本，不做全局 subtree observer；仅在出现“顶边贴近 0 的 fixed 浮层”时 patch `top`。

本目录记录“现状快照”，更完整的问题推导与历史路径见：

- `docs/MobileDevelopment.md`（6.3）
- `docs/MobileDynamicStyleSafeAreaPatch.md`（历史链路）

## 2. 端到端链路（Android）

### 2.1 Edge‑to‑edge 与系统栏编排（native）

入口：`src-tauri/gen/android/app/src/main/java/com/tauritavern/client/AndroidInsetsBridge.kt`

已落地行为：

- `WindowCompat.setDecorFitsSystemWindows(window, false)`：启用 edge‑to‑edge。
- 状态栏/导航栏透明；允许内容延伸到系统栏区域。
- `layoutInDisplayCutoutMode = SHORT_EDGES`：允许在刘海区域布局（由 safe‑area 变量约束可交互内容）。
- system bars behavior 使用 `BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE`。

沉浸模式：

- `power_user.mobile_immersive_fullscreen`（默认 `true`）通过 JS bridge 控制 native 是否 `hide()` system bars（见 §4）。

### 2.2 Inset 注入契约（native → WebView）

负责监听/计算的模块：

- `AndroidInsetsBridge`：监听 system bars + display cutout + IME。
- `WebViewReadinessPoller`：避免在 `about:blank` 或 `readyState=loading` 时注入导致变量丢失。
- `WebViewInsetsStyleApplier`：向 WebView 注入 helper，并把 insets 写入 CSS 变量。

CSS 变量（对前端的稳定契约）：

- `--tt-safe-area-top/right/left/bottom`：安全区 inset（px）。
- `--tt-ime-bottom`：输入法可见时的底部 inset（px），注入在 `#sheld`（优先）或 `html`。
- `--tt-base-viewport-height`：记录“无 IME 时”的基准 viewport 高度（用于稳定高度计算）。

关键语义（沉浸模式 + 刘海）：

- Android 沉浸模式下，`--tt-safe-area-*` **仍包含 `displayCutout`**（刘海/打孔）inset；system bars 若实际可见（例如手势拉出）则 safe‑area 会随可见 inset 变大。

## 3. 前端消费（CSS / JS）

### 3.1 CSS 变量默认值与跨平台兜底

`src/style.css` 提供默认值（iOS/浏览器主要依赖）：

- `--tt-safe-area-* = env(safe-area-inset-*, 0px)`
- `--tt-viewport-bottom-inset = max(var(--tt-safe-area-bottom), var(--tt-ime-bottom))`

补充兜底：

- `src/index.html` 在 `load`/`resize` 更新 `--doc-height = window.innerHeight`，供移动端高度计算 fallback 使用。

Android 说明：

- Android WebView 可能返回 `env(safe-area-*) = 0`，因此 **以 native 注入为准**（覆盖 root style 变量）。

### 3.2 主界面移动端布局（核心容器）

`src/css/mobile-styles.css` 消费上述变量，主要约束点：

- 顶部容器（如 `#top-settings-holder/#top-bar`）使用 `top: max(var(--tt-safe-area-top), 0px)` 并加入左右 padding。
- 主容器 `#sheld` 以 `safe-area-top + topBarBlockSize` 定位，并用 `--tt-base-viewport-height`/`--doc-height` 计算高度；底部通过 `--tt-safe-area-bottom` 与 `--tt-ime-bottom` 合成有效 bottom inset。

这些规则的目标是：edge‑to‑edge 打开后，**可交互内容仍避开刘海/系统栏/键盘**。

### 3.3 第三方脚本浮层：overlay safe‑area top 兜底（移动端）

实现：`src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js`  
安装入口：`src/tauri/main/bootstrap.js`（仅 Tauri mobile UA）

当前策略：

- **Admission**：仅观察 `document.body` 的直系子节点新增/移除（`subtree: false`）。
- **判定**：对 `position: fixed` 且计算后的 `top` 贴近 0（阈值范围内）的元素进行处理。
- **补丁**：对命中元素设置 `top: max(var(--tt-safe-area-top), <原top>) !important`。
- **排除**：明确跳过 `body/#sheld/#chat` 等核心容器（避免影响主界面）。
- **Revalidate**：监听 `html.style` 变化（native 注入会触发）+ `visualViewport`/`resize`/`orientationchange` 以重新校验 active set。

该控制器的边界是：只对“第三方顶层浮层贴顶”做最小修正，不承担全局样式重写职责。

### 3.4 旧 WebView JS 能力补齐（移动端）

实现：`src/tauri/main/compat/mobile/mobile-runtime-compat.js`

- 只在 Tauri mobile 安装，补齐少量缺失的标准 API（如 `Array.prototype.at` 等）。
- 通过 `window.__TAURITAVERN_MOBILE_RUNTIME_COMPAT__` sentinel 保证只执行一次。

## 4. 沉浸模式开关（Android）

前端入口：`src/scripts/mobile-system-ui.js`

- 通过 JS bridge `window.TauriTavernAndroidSystemUiBridge` 调用 native：
  - `setImmersiveFullscreenEnabled(boolean)`
  - `isImmersiveFullscreenEnabled()`

native 侧实现：`src-tauri/gen/android/app/src/main/java/com/tauritavern/client/AndroidSystemUiJsBridge.kt`

重要约束：

- **沉浸模式仅影响 system bars 的显示**；safe‑area 仍保留 `displayCutout`，避免刘海设备的交互元素进入不可用区域。

## 5. 已支持 / 明确不支持

已支持：

- Android edge‑to‑edge + safe‑area 变量注入（包含 IME）。
- Android 沉浸模式下保留 cutout safe‑area 语义。
- iOS `viewport-fit=cover` + `env(safe-area-inset-*)` 消费。
- 第三方脚本 fixed 浮层的 safe‑area top 元素级修正（移动端）。

明确不支持 / 不承诺：

- 不做第三方 `<style>` 文本 rewrite（风险高、成本高、回归面大）。
- overlay compat 不保证覆盖“非 body 直系子节点插入”的浮层（若未来出现真实样本，再数据驱动扩展观察点）。
- overlay compat 只处理 **top safe‑area**，不做通用的 left/right/bottom 兜底。

## 6. 最小回归与调试

建议最小回归：

1. Android（刘海机型）+ 沉浸模式：第三方脚本弹窗关闭按钮不进入刘海/状态栏区域。
2. 键盘弹出/收起：`#sheld` 高度与输入框不被遮挡。
3. 旋转屏幕：safe‑area 与布局重新校验无抖动回归。

快速调试点：

- `getComputedStyle(document.documentElement).getPropertyValue('--tt-safe-area-top')`
- `window.__TAURITAVERN_MOBILE_OVERLAY_COMPAT__` 是否已安装
- `window.__TAURITAVERN_MOBILE_RUNTIME_COMPAT__ === true`（旧 WebView）

