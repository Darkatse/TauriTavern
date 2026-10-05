# 移动端内容视口与窗口背景

移动端的系统栏、刘海和停靠键盘由原生宿主在主 WebView 的实际矩形上消费一次。网页内没有系统栏或键盘适配机制：第一方与扩展都按普通浏览器规则排版。

## 两份几何事实

| 事实 | 内容 | 变化时机 | 消费者 |
| --- | --- | --- | --- |
| 窗口快照 | 窗口物理像素尺寸、政策避让、scale、revision | 旋转、窗口尺寸、显示政策变化 | `#bg1` 窗口矩形、原生背景条带、第一方竖屏判断 |
| 内容视口 | 窗口减去政策避让与停靠键盘 | 上述变化，以及键盘出现、消失、高度变化 | 浏览器布局引擎 |

窗口快照不含键盘。键盘只改变主 WebView 的底边，顶部固定；键盘过渡不产生 JS 背景发布、`:root` 变量写入或图片工作。浮动与分离键盘不报告 inset，不缩小视口。

## Android

入口 `AndroidWindowLayout.kt`，由 `MainActivity` 编排生命周期，`WindowLayoutPlugin` 供 Rust 调用。

- edge-to-edge 只由 `WindowCompat.enableEdgeToEdge(window)` 配置。
- 政策避让：非沉浸为稳定的 systemBars ∪ displayCutout，沉浸为零。用户偏好 `power_user.mobile_immersive_fullscreen`（默认开启）经 `mobile-system-ui.js` 与 `AndroidSystemUiJsBridge` 传给原生。
- 主 WebView 矩形由一个函数计算：四边取政策避让，底边取政策避让与 IME 底部 inset 的较大值；相同矩形不重复提交。
- 提交时机：`onApplyWindowInsets` 更新目标，没有动画时直接提交；IME 动画期间 `onProgress` 提交当前高度，`onEnd` 按实际终态收敛。
- 内容根节点把原始 insets 传给子视图。主 WebView 自己的监听器把系统栏、刘海与 IME 清零：API 31+ 交给 WebView 的 `onApplyWindowInsets`；API 28–30 上它替换了 Chromium 构造时安装的监听器，引擎不接收 inset。
- IME 动画在 `onStart` 登记；`onPrepare` 只暂缓紧随其后的一轮布局，因为 API 30 上开始前被取消的动画不会再有 `onEnd`。
- 元素全屏是独立的展示状态：隐藏系统栏，由兄弟容器覆盖主页面，不改变用户偏好、窗口快照或 WebView 矩形。临时滑出的系统栏同样不改变政策。
- 旋转等由 `configChanges` 原位处理。Activity 重建（如切换开发者刘海选项）后 Tauri 插件仍持有旧 Activity，窗口插件拒绝请求，需重启应用。
- 窗口背景画在 decorView 的背景上：底色加条带。状态栏图标明暗由 `AndroidStatusBarAppearance` 对实际窗口像素取样。

## iOS

入口 `infrastructure/ios_webview.rs`，安装 `platform/ios_window_layout.rs` 的窗口宿主 UIView。

- WKWebView 上、左、右约束到 `safeAreaLayoutGuide`，底边约束到 `keyboardLayoutGuide.topAnchor`。键盘收起时 guide 停在底部安全区。
- iOS 没有沉浸模式，政策避让始终是安全区。
- 内部 UIScrollView 保持 `contentInsetAdjustmentBehavior = .never` 与零 inset；WebKit 自己的焦点、选区与键盘处理照常运行。
- 宿主在 `layoutSubviews` 按值去重发布窗口快照。底色设在宿主视图上，条带是插在 WKWebView 之下的 UIImageView。
- 元素全屏时 WebKit 会把 WKWebView 移出宿主；归还时宿主重新激活同一组约束。

## 鸿蒙

`EntryAbility` 设置 `setWindowLayoutFullScreen(false)`，系统让窗口留在安全区内，系统栏后不显示壁纸。键盘由 ArkWeb 处理。鸿蒙没有窗口快照，`util/window-layout.js` 中的第一方竖屏判断与订阅仍用 `matchMedia('(orientation: portrait)')`。

## 窗口背景

`src/scripts/window-backdrop.js` 的 `applyWindowBackdrop()` 是第一方背景的唯一写入点：全局背景、聊天锁定背景、聊天自定义与生成图片、填充方式、主题底色、自定义 CSS 与 OLED 开关都经过它。新增第一方背景来源也接入它。

`src/scripts/util/window-layout.js` 把窗口快照写成 `#bg1` 上的 `--tt-window-x/-y/-width/-height`。`#bg1` 固定为窗口矩形并使用 `background-attachment: scroll`，键盘不改变壁纸坐标。移动端壁纸切换不做渐变，两个渲染者始终呈现同一外观。

`set_window_backdrop` 的顺序：

1. JS 按帧合并，壁纸输入与底色分别去重；只改底色时不发送壁纸。`#bg1` 不可见（如 OLED）时清除壁纸。
2. 原生核对窗口 revision 后立即更新底色。壁纸变化时生成新令牌并清除旧条带，返回自己的窗口快照与令牌。这一步不等待解码，新选择立即使旧渲染失效。
3. `tt-adapter-media::window_backdrop` 一次只解码一张图：经 Host Resource Store 读取，取首帧并应用 EXIF 方向，按 `background-size`/`background-position` 映射到窗口，按预乘 alpha 采样，输出透明条带。
4. 原生在窗口 revision 与令牌都匹配时整体替换条带，过期结果直接丢弃。

系统栏条带不呈现：视频壁纸（含第一方 mp4）、渐变与外部 URL（只画底色并记录诊断）、弹窗遮罩、主题滤镜与第三方背景层。动画图片取首帧。条带不做磁盘缓存。

## 第一方行为

- `--doc-height` 是静态别名：支持时 `100dvh`，否则 `100vh`。
- 只有高度变化的 resize（键盘）不触发与宽度相关的工作：上游 resize 处理器在移动宿主上早退，聊天虚拟化度量按宽度门控，Quick Reply 编辑器只在宽度变化时 blur/focus。movingUI 缩放按宿主身份 `isMobileHost()` 判断。
- 聊天宽度：移动宿主且窗口竖屏时固定为 100%，滑块禁用；竖屏读窗口快照，不读视口。
- 输入框聚焦（`src/scripts/chat-input-focus.js`）：移动宿主拒绝导航与恢复类聚焦，只在编辑意图下弹出键盘；Android 进入后台时让输入框失焦。
- 第一方弹窗用自身类名的普通 CSS 管理尺寸；聊天列表自己负责滚动。
- 主题 CSS 按普通层叠生效，宿主不改写主题对第一方容器的几何。

## 与上游的本地差异

同步上游时保留这些删除：

- `src/index.html` 中写入 `--doc-height` 像素值的 resize 脚本。
- `src/style.css` 中 `html` 上的 transform 与 perspective：它们让 `html` 成为 fixed 元素的包含块，根文档滚动时 `#bg1` 会随之移动。
- `src/css/mobile-styles.css` 中 `#bg1` 的 `100dvw`/`100dvh !important` 尺寸：它会压过 `#bg1` 的窗口矩形。
- `src/scripts/browser-fixes.js` 中 iOS resize 时把根元素临时设为 `position: fixed` 的补偿。

公开布局 API 与旧变量的状态见 [API/Layout.md](../API/Layout.md)。
