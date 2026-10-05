# `window.__TAURITAVERN__.api.layout` — 内容视口

在 Android 与 iOS 上，宿主让主 WebView 只占据可交互区域：系统栏、刘海和停靠键盘已经从视口尺寸中扣除。扩展按普通浏览器规则排版即可，不需要识别宿主或扣除 inset。浮动与分离键盘不缩小视口。

```css
.my-panel {
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
}
.my-panel-content {
    flex: 1;
    min-height: 0;
    overflow: auto;
}
```

`100vh`、`100dvh`、`window.innerHeight`、`position: fixed`、`<dialog>` 和 iframe 都使用各自正常的浏览器坐标系。`--doc-height` 是静态 CSS 别名：支持时为 `100dvh`，否则为 `100vh`。

## 读取与订阅

```js
const layout = window.__TAURITAVERN__.api.layout;
const snapshot = layout.snapshot();
const unsubscribe = await layout.subscribe(snapshot => {
    console.log(snapshot.viewport.width, snapshot.viewport.height);
});
await unsubscribe();
```

宿主 `abiVersion: 2`；布局快照 `version: 2`，长度单位为 CSS px：

- `timestampMs`：读取时间。
- `viewport`：visual viewport 的 `{ left, top, width, height, right, bottom }`；没有 VisualViewport API 时读取布局视口。
- `safeFrame`：与 `viewport` 相同。
- `safeInsets`：`{ top: 0, right: 0, bottom: 0, left: 0 }`。
- `ime`：`{ bottom: 0, viewportBottomInset: 0, keyboardOffset: 0 }`。

`safeInsets` 与 `ime` 表示视口内仍需页面自行避让的量，宿主已全部消费，因此为零。订阅立即交付一次快照，之后把 window resize 与 visual viewport resize/scroll 按帧合并交付；最后一个订阅取消时移除监听。

## 系统栏区域

非沉浸模式下，系统栏后面显示宿主绘制的壁纸或主题底色。网页像素（包括遮罩、滤镜和视频）不会延伸到系统栏。

## 旧契约

- `ime.activeSurface` 与 `ime.kind` 已移除。
- `--tt-inset-*` 保留为 `env(safe-area-inset-*, 0px)` 的别名；`--tt-viewport-bottom-inset` 是 `--tt-inset-bottom` 的别名。
- `--tt-ime-bottom` 与 `--tt-base-viewport-height` 已移除。
- `data-tt-mobile-surface` 属性与 `layout-kit.js` 的 `SURFACE`/`applySurface()` 没有布局作用，只为已有调用保留。
- `layout-kit.js` 的 `getLayoutApi()`、`waitForHostReady()`、`subscribeLayout()` 继续可用。

使用旧键盘位移、spacer 或安全区补偿的扩展，删除这些代码，直接按内容视口排版。
