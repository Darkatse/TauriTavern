# `window.__TAURITAVERN__.api.layout` — Layout API（Safe‑Area / Viewport / IME）

TauriTavern 为扩展作者提供的布局 API：用**稳定契约（CSS vars + attributes）**统一移动端 safe‑area、viewport 与 Android IME（键盘）语义；并提供 `api.layout` 作为读取/订阅入口，外加可选 `layout-kit.js` DX 糖衣。

> **核心理念**：宿主负责“提供契约与最小准入（admission）”，扩展只需几行 opt‑in，就能稳定适配移动端；不引入新的运行时几何系统，不做黑名单堆积。

## 0. 快速上手（推荐：layout-kit.js）

`layout-kit.js` 是 TauriTavern 提供的“软 SDK（DX 糖衣）”：**推荐使用**，用于避免字符串拼写错误、减少样板代码。

它不会替代硬 ABI；底层仍然是 `data-tt-mobile-surface` + `--tt-*` contract。

### 0.1 安装（ESM import）

```js
import {
  waitForHostReady,
  subscribeLayout,
  SURFACE,
  applySurface,
} from '/scripts/tauritavern/layout-kit.js';

await waitForHostReady();
```

### 0.2 全屏面板 + 遮罩（移动端 safe‑area + IME 自动正确）

```js
applySurface(backdropEl, SURFACE.Backdrop);
applySurface(panelEl, SURFACE.FullscreenWindow);
```

> `fullscreen-window` 会进入宿主的 safe‑area/IME contract；`backdrop` 保持 full‑bleed（不收 safe‑area）。

### 0.3 悬浮球/小窗（避免被贴边限制）

```js
applySurface(bubbleEl, SURFACE.FreeWindow);
```

### 0.4 同源 iframe（viewport-host）

```js
applySurface(iframeEl, SURFACE.ViewportHost);
iframeEl.src = '/scripts/extensions/third-party/my-ext/ui/index.html';
```

### 0.5 订阅布局快照（用于 clamp / 自定义动画等）

```js
const unsubscribe = await subscribeLayout((snap) => {
  // snap.safeInsets / snap.safeFrame / snap.ime.keyboardOffset
});
```

---

## 0.x 不使用 SDK（直接写硬 ABI）

如果你不想引入 `layout-kit.js`（或无法使用 ESM import），仍然可以直接使用硬 ABI：

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const layout = window.__TAURITAVERN__.api.layout;

panelEl.dataset.ttMobileSurface = 'fullscreen-window';
backdropEl.dataset.ttMobileSurface = 'backdrop';
```

---

## 1. 硬 ABI（Contract）：扩展最推荐的接入方式

### 1.1 CSS 变量（stable）

这些变量是宿主对外承诺的布局输入（px）：

- `--tt-inset-top/right/left/bottom`：当前应避让的 safe‑area inset
- `--tt-base-viewport-height`：Android 下“无 IME 时”的基准 viewport 高度
- `--tt-viewport-bottom-inset`：底部可达性 inset（`max(--tt-inset-bottom, --tt-ime-bottom)` 的语义；但 Android IME 是 surface‑local，见 3.2）
- `--tt-ime-bottom`：Android 下键盘 inset（**注入到 active IME target surface root**，不承诺在 `:root` 全局更新）

> 说明：iOS 主要使用 `env(safe-area-inset-*)`，TauriTavern 在 `src/style.css` 中提供默认值；Android 以 native 注入为准。

### 1.2 `data-tt-mobile-surface` taxonomy（stable）

扩展可显式 opt‑in：

| 值 | 语义 | 适用场景 |
| --- | --- | --- |
| `backdrop` | full‑bleed 遮罩，不收 safe‑area | modal overlay mask |
| `fullscreen-window` | 全屏交互窗口，收四边 safe‑area | 全屏面板、弹窗根 |
| `free-window` | 可拖拽浮窗，宿主不长期钳制 top/left | 悬浮球、小工具窗 |
| `viewport-host` | 同源 iframe 宿主窗口（outer full‑bleed） | iframe 小应用（同源） |
| `edge-window` | 严格 fallback（不推荐显式使用） | 仅宿主兜底 |
| `none` | 显式声明不接入 | 极少数特殊场景 |

> 重要：宿主的 overlay classifier 保持克制（避免全局 subtree observer）；框架型扩展请优先显式写 `data-tt-mobile-surface`，不要“赌 classifier 一定扫到结构”。

---

## 2. `api.layout`（读取/订阅布局快照）

### 2.1 `layout.snapshot()`

```js
const snap = layout.snapshot();
console.log(snap.safeInsets.top, snap.viewport.height, snap.ime.keyboardOffset);
```

返回 `LayoutSnapshot`（字段均为 px number；`activeSurface` 为 Element 引用或 `null`）：

- `timestampMs`
- `viewport`：`{ left, top, width, height, right, bottom }`（优先 `visualViewport`）
- `safeInsets`：`{ top, right, bottom, left }`（来自 `:root` 的 `--tt-inset-*`）
- `safeFrame`：viewport 扣除 safeInsets 的 frame
- `ime`：
  - `activeSurface`：当前 active IME surface root（可能为 `null`）
  - `kind`：`'composer' | 'fixed-shell' | 'dialog'`
  - `bottom`：active surface 的 `--tt-ime-bottom`
  - `viewportBottomInset`：`max(safeInsets.bottom, bottom)`
  - `keyboardOffset`：`max(viewportBottomInset - safeInsets.bottom, 0)`

### 2.2 `layout.subscribe(handler)`

订阅会在以下信号变更时推送新快照：

- `resize/orientationchange`
- `visualViewport resize/scroll`（如果可用）
- `:root style` 变更（insets/base viewport）
- active IME surface 变更/其 style 变更（IME bottom）

```js
const unsubscribe = await layout.subscribe((snap) => {
  // 更新气泡 clamp / panel padding / 自定义动画等
});

// later
await unsubscribe?.();
```

约束（稳定契约）：
- handler 会**立即**收到一次快照（避免订阅后还要手动 snapshot）
- `unsubscribe` 幂等，可安全重复/延迟调用
- 错误不静默：非法参数或 DOM 不可用会直接抛错（便于排查）

---

## 3. IME（Android 键盘）语义要点

### 3.1 为什么 `--tt-ime-bottom` 是 surface‑local

Android 下 IME 不再依赖 WebView viewport resize（避免双重解释键盘语义）。宿主把 IME inset 注入到“当前正在输入的 surface root”，并通过 `data-tt-ime-*` 标记 active surface。

因此：
- 不要假设 `:root` 上会出现键盘高度
- 对全屏面板/弹窗，正确做法是：把面板 root 标记为 `fullscreen-window`（让宿主把它当作 IME target）
- 如果你只需要“UI 不被挡住”，通常不需要自己计算键盘高度

### 3.2 `keyboardOffset` 的意义

`keyboardOffset = max(imeViewportBottomInset - safeInsets.bottom, 0)` 表示“键盘相对 safe‑area 底部新增的遮挡高度”，适合用于：
- 自定义 bottom padding / scroll-padding
- 自己的 IME 进入/退出动画

---

## 4. `viewport-host`（same-origin iframe）

仅承诺 **same-origin iframe**。

推荐模式：

```js
iframe.dataset.ttMobileSurface = 'viewport-host';
iframe.src = '/scripts/extensions/third-party/my-ext/ui/index.html';
```

语义：
- outer `viewport-host` 永远 full‑bleed（不收 safe‑area，避免 replaced element shrink-to-fit）
- safe‑area contract 由 iframe 内消费（宿主会把 `--tt-inset-* / --tt-base-viewport-height` 桥接进 iframe 的 `:root`）

---

## 5. `layout-kit.js`（可选 DX 糖衣）

推荐使用 `layout-kit.js`（DX 糖衣），用于减少样板代码并避免字符串 typo：

```js
import { waitForHostReady, SURFACE, applySurface } from '/scripts/tauritavern/layout-kit.js';

await waitForHostReady();
applySurface(panelEl, SURFACE.FullscreenWindow);
```

说明：
- `layout-kit.js` 是软 SDK，不替代硬 ABI
- 它只封装 `api.layout` 与 surface opt‑in，不引入新的几何系统
