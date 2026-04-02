# 移动端样式适配现状（Edge‑to‑Edge / Safe‑Area / 沉浸模式）

本文档描述 **已经落地** 的移动端（Android / iOS）样式与布局适配现状，重点覆盖：

- Edge‑to‑edge（透明系统栏、刘海区域扩展）
- Safe‑area / IME inset 的注入与消费（CSS 变量契约）
- 沉浸模式（隐藏 system bars）下的 full-bleed 布局策略
- 第三方脚本注入浮层的 safe‑area top 兜底（元素级补丁）

## 1. 范围与结论

结论（当前实现的核心要点）：

1. **Insets 是“宿主提供的布局契约”**：前端布局只消费 `--tt-inset-*`；Android 由 native 监听 `WindowInsets` 并直接注入当前布局应避开的 inset（`--tt-inset-*`），iOS 以 CSS `env(safe-area-inset-*)` 提供 `--tt-inset-*`。
2. **Android 的 IME 是宿主语义，不再透传为 WebView viewport resize**：native 读取 IME inset 后只以 `--tt-ime-bottom` / `--tt-base-viewport-height` 提供给前端，避免一份键盘语义在 WebView 内再被解释一次。
3. **沉浸模式是 full-bleed 策略开关**：Android 沉浸（system bars 隐藏）时，`--tt-inset-*` 回落为 `0`，因此第一方顶部 UI 与第三方 fixed 浮层都允许沉入状态栏/刘海区域。
4. **第三方浮层通过 surface classifier 进入 CSS contract**：不重写 `<style>` 文本，不做全局 subtree observer；仅对“可能是顶层 fixed surface”的节点打上 `data-tt-mobile-surface`（并在 edge-window 场景写入 `--tt-original-top`），几何修正由 geometry firewall 的 CSS 规则完成。
5. **iOS 禁用 WKWebView 的自动 content inset 调整**：将 `scrollView.contentInsetAdjustmentBehavior = .never` 并清空 `contentInset/scrollIndicatorInsets`，确保 `window.innerHeight` 真正覆盖到全屏；safe-area 只通过 `env(safe-area-inset-*)` 交给前端消费。

本目录记录“现状快照”，更完整的问题推导与历史路径见：

- `docs/AndroidDevelopment.md`
- `docs/iOSDevelopment.md`
- `docs/MobileDynamicStyleSafeAreaPatch.md`（历史链路）

## 2. 端到端链路（Android）

### 2.1 Edge‑to‑edge 与系统栏编排（native）

入口：`src-tauri/gen/android/app/src/main/java/com/tauritavern/client/AndroidInsetsBridge.kt`

已落地行为：

- `WindowCompat.setDecorFitsSystemWindows(window, false)`：启用 edge‑to‑edge。
- 状态栏/导航栏透明；允许内容延伸到系统栏区域。
- `layoutInDisplayCutoutMode = SHORT_EDGES`：允许在刘海区域布局；是否避让由 `--tt-inset-*` 的当前策略决定。
- system bars behavior 使用 `BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE`。

沉浸模式：

- `power_user.mobile_immersive_fullscreen`（默认 `true`）通过 JS bridge 控制 native 是否 `hide()` system bars（见 §4）。

### 2.2 Inset 注入契约（native → WebView）

负责监听/计算的模块：

- `AndroidInsetsBridge`：监听 system bars + display cutout + IME。
- `WebViewReadinessPoller`：避免在 `about:blank` 时注入导致变量丢失；以 `#sheld` 存在作为“可注入”的最小前置条件（不依赖 `readyState`，避免启动早期 focus 竞态）。
- `WebViewInsetsStyleApplier`：向 WebView 注入 helper，并把 insets 写入 CSS 变量。

CSS 变量（对前端的稳定契约）：

- `--tt-inset-top/right/left/bottom`：布局消费的有效避让 inset（px）。
- `--tt-ime-bottom`：输入法可见时的底部 inset（px），注入在 **active imeTarget root**（默认回退 `#sheld`；避免把键盘动画扩散为 `:root` 级全局样式失效）。
- `--tt-base-viewport-height`：记录“无 IME 时”的基准 viewport 高度（用于稳定高度计算）。

关键语义（沉浸模式 + 刘海）：

- Android 非沉浸模式下，`--tt-inset-*` 反映当前布局应避开的可见/稳定 safe area；
- Android 沉浸模式下，`--tt-inset-*` 回落为 `0`，应用以 full-bleed 方式覆盖到状态栏/刘海区域。
- Android IME 不再向 descendant WebView 继续透传为 viewport resize；页面内键盘位移只由 active surface 上的 `--tt-ime-bottom` 驱动。

## 3. 前端消费（CSS / JS）

### 3.1 CSS 变量默认值与跨平台兜底

`src/style.css` 提供默认值（iOS/浏览器主要依赖）：

- `--tt-inset-* = env(safe-area-inset-*, 0px)`（iOS）
- `--tt-viewport-bottom-inset = max(var(--tt-inset-bottom), var(--tt-ime-bottom))`

补充兜底：

- `src/index.html` 在 `load`/`resize` 更新 `--doc-height = window.innerHeight`，供移动端高度计算 fallback 使用。

Android 说明：

- Android WebView 可能返回 `env(safe-area-*) = 0`，因此 **以 native 注入为准**（覆盖 root style 变量）。

### 3.2 主界面移动端布局（核心容器）

`src/css/mobile-styles.css` 消费上述变量，主要约束点：

- 顶部容器（如 `#top-settings-holder/#top-bar`）使用 `top: max(var(--tt-inset-top), 0px)` 并加入左右 padding。
- 为避免主题 `custom_css` 直接覆盖移动端核心几何，宿主会注入一个 **host-last geometry firewall**（永远位于 `#custom-style` 之后）：
  - 实现：`src/tauri/main/compat/mobile/mobile-geometry-firewall.js`
  - 产物：`<style id="tt-mobile-geometry-firewall">`（keep-last，确保始终为 `<head>` 最后一个 element）
  - 覆盖范围：只收回核心几何属性（`#top-settings-holder/#top-bar/#top-settings-holder > .drawer > .drawer-content:not(.fillLeft):not(.fillRight)/#sheld/#form_sheld`），不干预主题 skin
- 第一方顶部设置面板（`#top-settings-holder` 下的非侧栏 drawer）不再依赖运行时测量与 inline 回写：
  - 由 geometry firewall 以 CSS contract 直接约束几何（holder-anchored），避免出现第二/第三套几何系统
- 主容器 `#sheld` 以 `inset-top + topBarBlockSize` 定位，并用 `--tt-base-viewport-height`/`--doc-height` 计算高度。
- Android 的键盘抬升不再直接绑定在主题可覆写的 `#form_sheld` 上；宿主使用 host-private DOM + contract 承载：
  - `src/tauri/main/compat/mobile/android-ime-layout-host.js` 在 `#form_sheld` 内安装 lift/spacer 节点（仅 composer）。
  - fixed-shell 等非 composer surface 由 geometry firewall 直接消费 `--tt-ime-bottom`（见 §3.6）。

这些规则的目标是：在非沉浸模式下避开顶部/底部安全区与键盘，在沉浸模式下保持 full-bleed。

### 3.3 第三方脚本浮层：surface classifier + safe‑area contract（移动端）

实现：

- 分类/契约输出：`src/tauri/main/compat/mobile/mobile-overlay-surface-admission.js`
- 观察与有界 settle window：`src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js`
- 同源 iframe contract bridge：`src/tauri/main/compat/mobile/mobile-iframe-viewport-contract-bridge.js`

安装入口：`src/tauri/main/bootstrap.js`（仅 Tauri mobile UA）

当前策略：

- **Admission**：仅观察 `document.body` 的直系子节点新增/移除（`subtree: false`），并对带 `script_id` 的 portal root 进一步扫描其子树（JS-Slash-Runner 常见挂载形态）。
- **判定**：对符合条件的 `position: fixed` 节点进行 surface 分类（backdrop / viewport-host / fullscreen-window / free-window / edge-window）。
- **输出**：不再直接写入 `top`；改为输出契约属性：
  - `data-tt-mobile-surface="backdrop|viewport-host|fullscreen-window|free-window|edge-window"`
  - `data-tt-mobile-surface-admitted="1"`（host-private sentinel，用于区分 host-admitted 与显式 opt-in；非 ABI）
  - `--tt-original-top=<px>`（仅 edge-window，用于在 safe-area top 之上保持原始 top 偏移）
- **落地**：几何修正主要由 geometry firewall 的 CSS contract 执行（`free-window` 例外：仅 admission-time 允许一次性 nudge 初始 top，之后不再接管）：
  - `[data-tt-mobile-surface="edge-window"]`：只修正 top
  - `[data-tt-mobile-surface="fullscreen-window"]`：修正四边并把 width/height 改成 auto（避免 `100vh` 把底部顶出屏幕）
  - `[data-tt-mobile-surface="viewport-host"]`：outer host 强制 full-bleed（不做 safe-area 收缩；safe-area contract 进入 document boundary 处理）
  - `[data-tt-mobile-surface="free-window"]`：不接管 `top/left`（仅 admission-time 允许一次性把初始位置从 safe-area 顶部挪开）
  - `[data-tt-mobile-surface="backdrop"]`：保持 full-bleed（不做 inset）
  - 备注：firewall 的 surface selector 会刻意重复 attribute 以获得足够 specificity（覆盖常见框架 scoped CSS + `!important`）
- **排除**：明确跳过 `body/#sheld/#chat` 等核心容器（避免影响主界面）。
- **显式 opt-in**：若节点已带 `data-tt-mobile-surface`，该控制器将尊重并不再改写（便于第三方脚本作者自我修复）。
- **Revalidate**：停止自动高频重分类（不再监听 `style/class` 与 `visualViewport`/`resize`/`orientationchange` 噪声）；仅在节点新增/移除时对新增子树做一次 admission。`controller.revalidate()` 保留为手动兜底（debug 用）。

补充：portal host 常见为全屏容器（有时 `pointer-events: none`），实际交互面板通过 portal/render 落到其内部；classifier 会优先准入真实可交互 surface（避免 host 被误当作唯一 surface）。

该控制器的边界是：只负责发现与分类“可能需要 safe-area 约束的第三方顶层 surface”，并输出最小属性契约，不承担全局样式重写职责；在沉浸模式下由于 `--tt-inset-top = 0`，对应的 geometry contract 会自然退化为 full-bleed。

### 3.4 旧 WebView JS 能力补齐（移动端）

实现：`src/tauri/main/compat/mobile/mobile-runtime-compat.js`

- 只在 Tauri mobile 安装，补齐少量缺失的标准 API（如 `Array.prototype.at` 等）。
- 通过 `window.__TAURITAVERN_MOBILE_RUNTIME_COMPAT__` sentinel 保证只执行一次。

### 3.5 聊天输入框焦点策略（移动端）

实现：`src/scripts/chat-input-focus.js`

当前策略：

- `#send_textarea` 的程序化聚焦按意图分为 `navigation` / `restoration` / `editing`。
- 移动端会拒绝 `navigation` 与 `restoration`，因此切角色、读历史聊天、welcome screen 创建临时聊天、按钮回焦都不会自动把键盘弹起。
- 显式编辑流仍允许聚焦，例如消息编辑收尾、Quick Reply 把内容注入聊天输入框后继续编辑。
- Tauri Android 在文档进入 `hidden` 时，若 `#send_textarea` 仍持有焦点，会主动 `blur()` 并清空 restoration 状态；因此从系统后台返回时不会因为旧焦点被恢复而自动弹出键盘。
- 该策略完全留在前端共享模块，不依赖 native/WebView 对 `focus()` 做拦截。

### 3.6 Android IME ownership 路由（surface-local contract）

实现：

- JS focus 路由：`src/tauri/main/compat/mobile/mobile-ime-surface-controller.js`
- bridge target：`src-tauri/gen/android/app/src/main/java/com/tauritavern/client/WebViewInsetsStyleApplier.kt`
- fixed-shell 消费：`src/tauri/main/compat/mobile/mobile-geometry-firewall.js`

当前策略（Android）：

- 监听 `focusin/focusout`（capture），解析“当前正在输入的 surface root”，并写入 host-private attributes：
  - `data-tt-ime-active`
  - `data-tt-ime-surface="composer|fixed-shell|dialog"`
- 调用 `window.__TAURITAVERN_INSETS__.setImeTarget(rootOrNull)` 将 `--tt-ime-bottom` 注入到 active root（`#sheld` 使用默认回退，因此传 `null`）。
- composer（`#sheld/#form_sheld`）继续由 `android-ime-layout-host` 的 lift/spacer 消费键盘偏移（不扩散到其它界面）。
- fixed-shell（角色编辑、world/editor drawer 等）由 geometry firewall 通过 `height/max-height/bottom + scroll-padding-bottom` 消费 `--tt-ime-bottom`，避免输入被键盘遮挡。

备注：

- iOS 主要依赖 viewport resize；`--tt-ime-bottom` 可能始终为 `0`，但上述策略不应破坏布局。

## 4. 沉浸模式开关（Android）

前端入口：`src/scripts/mobile-system-ui.js`

- 通过 JS bridge `window.TauriTavernAndroidSystemUiBridge` 调用 native：
  - `setImmersiveFullscreenEnabled(boolean)`
  - `isImmersiveFullscreenEnabled()`

native 侧实现：`src-tauri/gen/android/app/src/main/java/com/tauritavern/client/AndroidSystemUiJsBridge.kt`

重要约束：

- **沉浸模式不仅影响 system bars 的显示，也切换布局策略**；启用后顶部 safe-area 归零，允许 full-bleed 布局。

## 5. 已支持 / 明确不支持

已支持：

- Android edge‑to‑edge + inset 契约变量（包含 IME）。
- Android 沉浸模式下以 full-bleed 策略运行，顶部 inset 不再额外避让刘海/状态栏。
- iOS `viewport-fit=cover` + `env(safe-area-inset-*)` 提供 `--tt-inset-*`。
- 第三方脚本 fixed 浮层的 inset top 元素级修正（移动端）。
- Android：IME ownership 路由（composer + fixed-shell），避免把键盘动画扩散为全局 `:root` 变量更新。
- 聊天导航类场景不再自动聚焦 `#send_textarea`，Tauri Android 从系统后台恢复时也不会恢复聊天输入焦点；移动端键盘只在真正进入输入/编辑意图时弹出。

明确不支持 / 不承诺：

- 不做第三方 `<style>` 文本 rewrite（风险高、成本高、回归面大）。
- overlay compat 不保证覆盖“非 body 直系子节点插入”的浮层（若未来出现真实样本，再数据驱动扩展观察点）。
- overlay compat 只处理 **top safe‑area**，不做通用的 left/right/bottom 兜底。

## 6. 最小回归与调试

建议最小回归：

1. Android（刘海机型）+ 沉浸模式：第一方顶部 UI 与第三方脚本浮层允许进入刘海/状态栏区域。
2. 键盘弹出/收起：`#sheld` 高度与输入框不被遮挡。
3. 旋转屏幕：safe‑area 与布局重新校验无抖动回归。

快速调试点：

- `getComputedStyle(document.documentElement).getPropertyValue('--tt-inset-top')`
  - 沉浸模式期望接近 `0px`
  - 非沉浸模式期望反映当前顶部 safe area
- `window.__TAURITAVERN_MOBILE_OVERLAY_COMPAT__` 是否已安装
- `window.__TAURITAVERN_MOBILE_RUNTIME_COMPAT__ === true`（旧 WebView）
- `window.__TAURITAVERN_INSETS__` 是否存在（`apply/setImeTarget/reapply`）
- 当前 active surface 是否正确打标：`[data-tt-ime-active][data-tt-ime-surface]`
