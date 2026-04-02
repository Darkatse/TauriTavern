# `window.__TAURITAVERN__.api.chat` — API 参考

TauriTavern 为“记忆/数据库/检索类扩展”扩展开发者提供的增强 API。

> **核心理念**：把 SillyTavern 扩展开发中最常见的脏活累活——找消息、搜内容、存数据——变成一行 API 调用。Rust 后端处理重活，JS 侧只管调接口。

## 0. 快速上手

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const api = window.__TAURITAVERN__.api.chat;
const handle = api.current.handle();
```

> **窗口化聊天**：TauriTavern 前端只加载最近 N 条消息（windowed payload），`getContext().chat` 仅反映当前窗口。下面的 API 会透明地穿越窗口边界，在 Rust 后端访问完整历史——你无需关心分页细节。

## 1. 核心入口

| 方法 | 返回值 | 说明 |
| --- | --- | --- |
| `api.chat.open(ref)` | `ChatHandle` | 打开指定聊天 |
| `api.chat.current.ref()` | `ChatRef` | 当前聊天的引用 |
| `api.chat.current.handle()` | `ChatHandle` | 当前聊天的操作句柄 |
| `api.chat.current.windowInfo()` | `Promise<WindowInfo>` | 当前窗口状态信息 |

**`ChatRef` 类型**：

- 角色聊天：`{ kind: 'character', characterId, fileName }`
- 群聊：`{ kind: 'group', chatId }`

**`WindowInfo` 类型**：

| 字段 | 说明 |
| --- | --- |
| `mode` | `'windowed' \| 'off'` |
| `totalCount` | 聊天总消息数（不含 header） |
| `windowStartIndex` | 当前窗口起始的绝对索引（0-based） |
| `windowLength` | 当前窗口消息数 |

**索引映射示例**：

```js
const info = await api.current.windowInfo();
const absIndex = info.windowStartIndex + windowIndex;
```

## 2. ChatHandle 能力

通过 `api.open(ref)` 或 `api.current.handle()` 获取 `ChatHandle`。

---

### `locate.findLastMessage()` — 精准定位，不再手动遍历

> 💡 **解决的痛点**：SillyTavern 中定位最后一条包含特定数据的消息，需要 `chat.slice().reverse().find(...)` 手动遍历，耗时且存在窗口边界问题。

```js
const hit = await handle.locate.findLastMessage({
  role: 'assistant',                              // 可选：限定角色
  hasExtraKeys: ['TavernDB_ACU_IsolatedData'],     // 可选：消息必须包含的 extra 键
  scanLimit: 2000,                                 // 可选：最多扫描多少条
});
```

**返回值**：
- 命中：`{ index, message }` — 绝对索引 + 原始消息对象
- 未命中：`null`

全部由 Rust 后端高效执行——即使 10000 条消息也毫无压力。

---

### `searchMessages()` — 内置全文检索

> 💡 **解决的痛点**：SillyTavern 没有内置的消息搜索 API，扩展开发者只能手写 `filter/includes` 扫描，CJK 内容更是无法正常匹配。

```js
const hits = await handle.searchMessages({
  query: '是啊，吃什么',
  limit: 20,
  filters: {
    role: 'assistant',          // 可选：限定角色
    startIndex: 100,            // 可选：起始索引
    endIndex: 5000,             // 可选：结束索引
    scanLimit: 5000,            // 可选：从尾部向前最多扫描条数
  },
});
```

**返回 `SearchHit[]`**：

| 字段 | 说明 |
| --- | --- |
| `index` | 绝对索引（0-based） |
| `score` | 匹配评分（0~1，越大越匹配） |
| `snippet` | 可直接用于 UI 展示的短片段 |
| `role` | `'user' \| 'assistant' \| 'system'` |
| `text` | 命中消息的完整 `mes` 文本 |

**技术特点**：
- 基于片段命中评分 + TopK 召回（非向量检索，轻量高效）
- CJK / 无空格文本自动 bigram 分词，大幅提升中日韩文匹配率
- `scanLimit` 控制性能上限，移动端友好

---

### `store.*` — 扩展数据持久化

> 💡 **解决的痛点**：SillyTavern **从未提供** 扩展数据持久化方案。开发者只能把数据塞进消息体（放大 payload、数据耦合），或 hack 写入 `chat_metadata`（容量有限、语义不清）。

TauriTavern 提供每个聊天独立的 KV JSON 存储：

```js
// 写入
await handle.store.setJson({ namespace: 'my-ext', key: 'index', value: largeData });

// 读取
const data = await handle.store.getJson({ namespace: 'my-ext', key: 'index' });

// 更新（对象会深度合并；非对象会直接替换）
await handle.store.updateJson({
  namespace: 'my-ext',
  key: 'index',
  value: { lastFloor: 42, updatedAt: Date.now() },
});

// 重命名 key
await handle.store.renameKey({
  namespace: 'my-ext',
  key: 'index',
  newKey: 'index-v2',
});

// 列出所有键
const keys = await handle.store.listKeys({ namespace: 'my-ext' });

// 删除
await handle.store.deleteJson({ namespace: 'my-ext', key: 'old-key' });
```

兼容：`updateJSON()` 是 `updateJson()` 的别名；`updateKey()` 是 `renameKey()` 的别名。

**适用场景**：表格、索引、数据库快照等大 JSON 数据。数据与消息彻底解耦，不会膨胀聊天文件。

---

### `metadata.*` — 轻量配置存储

适合存储进度、配置项、短摘要等小状态：

```js
// 读取 chat 元数据
const meta = await handle.metadata.get();

// 写入扩展配置
await handle.metadata.setExtension({ namespace: 'my-ext', value: { lastFloor: 42 } });

// 删除（value 设为 null）
await handle.metadata.setExtension({ namespace: 'my-ext', value: null });
```

数据存储在 `chat_metadata.extensions[namespace]` 中，跨端可迁移，开销稳定。

---

### `summary()` / `stableId()`

```js
// 获取聊天摘要（不需要加载完整消息）
const summary = await handle.summary({ includeMetadata: true });

// 获取可持久化的稳定 ID
const id = await handle.stableId();
```

---

### `history.*` — 按需分页读取

需要遍历历史消息时使用（不会一次性把全量数据载入 JS）：

```js
let page = await handle.history.tail({ limit: 100 });
while (page.hasMoreBefore) {
  // page.messages: ChatMessage[]
  // page.startIndex: 本页首条消息的绝对索引
  page = await handle.history.before(page, { limit: 100 });
}
```

| 方法 | 说明 |
| --- | --- |
| `history.tail({ limit })` | 获取最新的 N 条消息 |
| `history.before(page, { limit })` | 向前翻一页 |
| `history.beforePages(page, { limit, pages })` | 一次拉多页（减少 IPC 往返，移动端推荐） |

返回值包含 `startIndex`（本页首条消息的绝对索引，0-based）和 `hasMoreBefore`（是否还有更早的消息）。
