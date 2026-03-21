# 从 SillyTavern 扩展适配到 TauriTavern（记忆类扩展指南）

本指南面向“记忆/数据库/召回”类扩展作者，目标是把对 `getContext().chat`（全量数组假设）的依赖迁移到：

- `window.__TAURITAVERN__.api.chat`（唯一入口）

从而在 windowed payload 下仍保持正确性与性能（尤其是移动端）。

## 0. 背景：为什么必须迁移

TauriTavern 默认启用 windowed payload：前端只加载最近 N 条消息。

这意味着：

- `getContext().chat.length` 不再代表全量楼层数
- `getContext().chat[i]` 的 `i` 是“窗口索引”，不是“绝对楼层索引”
- 扩展里常见的 `slice/map/filter/reverse` 全量扫描会直接失真或性能崩溃

## 1. 最小迁移套路（强烈推荐）

### 1.1 等待宿主 ready

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const api = window.__TAURITAVERN__.api.chat;
const handle = api.current.handle();
```

### 1.2 “楼层语义”迁移：用 `windowInfo()`

```js
const info = await api.current.windowInfo();
// 全量楼层数（不含 header）
const total = info.totalCount;
// 将 window index 映射为绝对 index
const absIndex = info.windowStartIndex + windowIndex;
```

迁移建议：

- 任何需要持久化跟踪的进度（`lastProcessedFloor`）都存**绝对索引**
- 永远不要把“窗口索引”写入持久化状态

### 1.3 历史读取：用 `history.tail/before/beforePages`

替代：

- `context.chat.slice(-N)` / `context.chat.map(...)`

使用：

```js
let page = await handle.history.tail({ limit: 100 });
while (page.hasMoreBefore) {
  // page.messages: ChatMessage[]
  page = await handle.history.before(page, { limit: 100 });
}
```

移动端建议：

- 用 `beforePages()` 拉多页减少 IPC：

```js
const pages = await handle.history.beforePages(page, { limit: 200, pages: 5 });
```

### 1.4 “从后往前找最后一条状态消息”：用 `locate.findLastMessage()`

替代：

- `context.chat.slice().reverse().find(...)`

使用：

```js
const hit = await handle.locate.findLastMessage({
  role: 'assistant',
  hasExtraKeys: ['TavernDB_ACU_IsolatedData'],
  scanLimit: 2000,
});
```

### 1.5 状态持久化：优先 `metadata` / `store`

推荐策略：

- 小状态（进度、配置、短摘要）→ `metadata.setExtension({ namespace, value })`
- 大状态（表格/索引/数据库）→ `store.setJson({ namespace, key, value })`

不要再做：

- 把大 JSON 塞进最后一条消息，然后 `saveChat()`（会放大 payload，且窗口化下定位成本很高）

### 1.6 召回/检索：用 `searchMessages()`

替代：

- 扩展侧对 `chat` 的关键词扫描（尤其是 CJK 场景）

使用：

```js
const hits = await handle.searchMessages({
  query: '关键词 或 中文短语',
  limit: 20,
  filters: {
    role: 'assistant',
    scanLimit: 5000,
  },
});
```

要点：

- `scanLimit` 是移动端的“性能上限开关”，建议总是设置
- `startIndex/endIndex` 可用于把检索限制在某个范围（例如只检索最近 2k 楼）

## 2. 常见替换对照表

| 上游 SillyTavern 用法 | TauriTavern 推荐替代 |
| --- | --- |
| `getContext().chat.length` | `await api.current.windowInfo().totalCount` |
| `chat[chat.length - 1]` | `await handle.history.tail({ limit: 1 })` |
| `chat.slice(-N)` | `await handle.history.tail({ limit: N })` |
| “倒序找最后状态” | `await handle.locate.findLastMessage({ ... })` |
| 把扩展状态塞进消息体 | `handle.store.setJson(...)` / `handle.metadata.setExtension(...)` |
| 关键词扫描召回 | `await handle.searchMessages({ query, limit, filters })` |

## 3. 你应该坚持的移动端约束

- 不要把全量历史对象数组塞回 JS（哪怕你能读到）
- 不要在高频事件里做 O(N) 扫描（`slice/map/filter/reverse`）
- 让 Rust 后端承担：定位、检索、按需分页读取

