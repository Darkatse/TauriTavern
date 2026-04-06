# TauriTavern Extension Development Guide

本文档面向 **SillyTavern 扩展开发者**，介绍 TauriTavern 提供的专属 API——让你的扩展更强大、更优雅。

> **一句话总结**：TauriTavern 为扩展提供了 `findLastMessage()` 精准定位、`searchMessages()` 全文检索、以及 `store.*` 状态持久化——这些都是 SillyTavern 从未内置，而开发者一直在手动造轮子的能力。

---

## 为什么值得适配？

在 SillyTavern 中，扩展开发者长期面临这些痛点：

| 痛点 | 传统做法 | TauriTavern 方案 |
| --- | --- | --- |
| **找最后一条特定消息** | `chat.slice().reverse().find(...)` 手动遍历 | ✅ `findLastMessage()` 一行搞定，后端高效定位 |
| **搜索历史消息** | 手写关键词扫描，CJK 断词全靠运气 | ✅ `searchMessages()` 内置全文检索，CJK 优化 |
| **扩展数据持久化** | 塞进消息体、手动 `saveChat()`，数据与消息耦合 | ✅ `store.*` 独立 KV 存储，干净可靠 |
| **小配置/进度保存** | 各种 hack 写入 `chat_metadata` | ✅ `metadata.*` 标准化接口，语义清晰 |

** TauriTavern，让你的扩展制作更简单。**

---

## 关于窗口化聊天

TauriTavern 采用**窗口化加载**（windowed payload）的设计：前端只保留最近 N 条消息在内存中，而非一次性加载全部聊天记录。这让长对话和移动端体验更流畅，同时也意味着 `getContext().chat` 只反映当前窗口内的消息。

你**不需要**关心底层细节——上面提到的 `findLastMessage()`、`searchMessages()`、`history.*` 等 API 已经帮你透明地处理了窗口边界，后端会扫描完整历史。只需调用 API，其余交给 Rust。

---

## 1. 快速开始

### 1.1 扩展目录

TauriTavern 的 third-party 扩展目录与 SillyTavern 兼容：

- local：`data/default-user/extensions/<folder>`
- global：`data/extensions/third-party/<folder>`
- 同名时 **local 优先**

资源端点：`/scripts/extensions/third-party/<folder>/<path>`

> 更多细节：[ThirdPartyExtensions.md](docs/CurrentState/ThirdPartyExtensions.md)

### 1.2 等待宿主就绪

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const api = window.__TAURITAVERN__.api.chat;
```

TauriTavern 在宿主层暴露 `window.__TAURITAVERN__`，你只需等待 `ready` 即可使用所有 API。

### 1.3 判断是否在 TauriTavern 中运行

```js
if (window.__TAURITAVERN__) {
  // TauriTavern 环境，可以使用增强 API
} else {
  // 标准 SillyTavern 环境，走原有逻辑
}
```

这样你的扩展可以轻松做到**两端兼容**。

---

## 2. 核心能力一览

TauriTavern 专属 API 的唯一入口：

```
window.__TAURITAVERN__.api.chat
```

### `findLastMessage()` — 再也不用手动遍历

以前为了找最后一条包含特定扩展数据的消息，你得：

```js
// ❌ 以前的做法
const hit = context.chat.slice().reverse().find(
  msg => msg.extra?.TavernDB_ACU_IsolatedData
);
```

现在只需要：

```js
// ✅ TauriTavern
const hit = await handle.locate.findLastMessage({
  role: 'assistant',
  hasExtraKeys: ['TavernDB_ACU_IsolatedData'],
  scanLimit: 2000,
});
// hit = { index, message } 或 null
```

后端 Rust 处理，快速、高效、准确。

### `searchMessages()` — 内置全文检索，告别手写搜索

以前你要在聊天记录里搜关键词？只能自己 `filter/includes`，CJK 用户更是一言难尽。

现在：

```js
// ✅ TauriTavern
const hits = await handle.searchMessages({
  query: 'AUV，您吉祥',
  limit: 20,
  filters: { role: 'assistant' },
});
// hits = [{ index, score, snippet, role, text }, ...]
```

- 内置 CJK bigram 分词优化
- 支持按角色、索引范围过滤
- `scanLimit` 控制性能开销

### `store.*` — 真正的扩展数据持久化

这是 **SillyTavern 从未实现** 的功能。以前存数据的"标准做法"是塞进消息体：

```js
// ❌ 以前的做法：把数据塞进最后一条消息
lastMsg.extra.myExtensionData = hugeJsonObject;
saveChat(); // 放大 payload，容易出问题
```

现在你有独立的 per-chat KV 存储：

```js
// ✅ TauriTavern
await handle.store.setJson({ namespace: 'my-ext', key: 'index', value: data });
const data = await handle.store.getJson({ namespace: 'my-ext', key: 'index' });
const keys = await handle.store.listKeys({ namespace: 'my-ext' });
await handle.store.deleteJson({ namespace: 'my-ext', key: 'old-key' });
```

数据与消息彻底解耦，干净、可靠、不膨胀 payload。

### `metadata.*` — 轻量配置存储

适合存储进度、配置项、短摘要等小状态：

```js
await handle.metadata.setExtension({ namespace: 'my-ext', value: { lastFloor: 42 } });
const meta = await handle.metadata.get();
```

---

## 3. 完整 API 参考

详细参数与返回值请查阅：

- 📖 **[API 参考](docs/API/Chat.md)** — 完整接口说明
- 🔄 **[适配指南](docs/API/Migration.md)** — 从 SillyTavern 扩展快速适配

---

## 4. 移动端性能建议

TauriTavern 同时运行在桌面和移动端（Android/iOS），以下建议让你的扩展在移动端也流畅运行：

- 检索和定位交给后端 API（`findLastMessage` / `searchMessages`），避免在 JS 侧做 O(N) 扫描
- 大状态用 `store.setJson()` 持久化，不要塞进消息体，小状态优先 `metadata.*`
- 使用 `scanLimit` 控制扫描上限，保护低端设备性能

---

## 5. 移动端布局适配（Safe‑Area / Viewport / IME）

如果你的扩展包含悬浮球、全屏面板、弹窗、同源 iframe 小应用等 UI，推荐接入 TauriTavern 的 Layout ABI：

- ✅ **推荐（DX）**：直接使用 `/scripts/tauritavern/layout-kit.js`（减少样板代码、避免字符串 typo）
- ✅ **硬 ABI（长期稳定）**：`data-tt-mobile-surface` + `--tt-inset-* / --tt-base-viewport-height / --tt-ime-bottom`
- ✅ **稳定 API**：`window.__TAURITAVERN__.api.layout`（读取/订阅 safe-area、viewport、IME 快照；layout-kit 内部也会用它）

### 5.1 几行接入：把你的 UI 归类为 surface

全屏面板：

```js
import { waitForHostReady, SURFACE, applySurface } from '/scripts/tauritavern/layout-kit.js';

await waitForHostReady();
applySurface(panelEl, SURFACE.FullscreenWindow);
applySurface(backdropEl, SURFACE.Backdrop);
```

悬浮球/小窗：

```js
applySurface(bubbleEl, SURFACE.FreeWindow);
```

同源 iframe（只承诺 same-origin）：

```js
applySurface(iframe, SURFACE.ViewportHost);
iframe.src = '/scripts/extensions/third-party/my-ext/ui/index.html';
```

> 提示：宿主的 overlay classifier 会保持克制（避免全局 subtree observer）。框架型扩展请优先显式写入 `data-tt-mobile-surface`，不要依赖宿主“猜中”你的结构。

### 5.2 订阅布局快照（用于 clamp / 自定义动画等）

```js
import { subscribeLayout } from '/scripts/tauritavern/layout-kit.js';

const unsubscribe = await subscribeLayout((snap) => {
  // snap.safeInsets / snap.safeFrame / snap.ime.keyboardOffset
});
```

完整参考：
- 📖 **[Layout API](docs/API/Layout.md)** — `api.layout` + mobile surface contract 说明
