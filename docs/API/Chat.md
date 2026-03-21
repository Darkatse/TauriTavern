# `window.__TAURITAVERN__.api.chat`

这是 TauriTavern 对“记忆/数据库/检索类扩展”提供的宿主专属 API。

设计目标：

- 在 **windowed payload**（聊天记录分片加载）下，仍然让扩展拿到稳定的“楼层语义”（绝对索引）
- 历史按需读、后端定位/检索、稳定的持久化接口
- 纯文本检索优先（不向量），并对 CJK 做专门优化

> 重要：该 API 是 TauriTavern 独有能力的**唯一入口**，不提供 `getContext().tauritavern` 或 `TauriTavern.getContext()` 之类 alias。

## 0. Ready（扩展侧建议）

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const api = window.__TAURITAVERN__.api.chat;
```

## 1. 核心入口

- `window.__TAURITAVERN__.api.chat.open(ref) -> ChatHandle`
- `window.__TAURITAVERN__.api.chat.current.ref() -> ChatRef`
- `window.__TAURITAVERN__.api.chat.current.handle() -> ChatHandle`
- `window.__TAURITAVERN__.api.chat.current.windowInfo() -> Promise<WindowInfo>`

`ChatRef`：

- 角色聊天：`{ kind: 'character', characterId, fileName }`
- 群聊：`{ kind: 'group', chatId }`

`WindowInfo`：

- `mode: 'windowed' | 'off'`
- `totalCount`: 当前 chat 的总消息数（不含 header）
- `windowStartIndex`: 当前前端窗口起始的**绝对消息索引**（0-based）
- `windowLength`: 当前前端窗口消息数

当你拿到上游事件里的 “window index”（例如 `MESSAGE_*` 的 `messageId`），映射到绝对索引：

```js
const info = await api.current.windowInfo();
const absIndex = info.windowStartIndex + windowIndex;
```

## 2. ChatHandle 能力

从 `api.open(ref)` 或 `api.current.handle()` 得到 `ChatHandle`，它代表“某一个具体 chat”。

### 2.1 `summary()` / `stableId()`

- `handle.summary({ includeMetadata? })`
  - 读取 chat 摘要，不加载全量 payload
- `handle.stableId()`
  - 获取可用于持久化跟踪的稳定 ID（character chat 基于 header 的 `integrity`；group chat 直接为 `chatId`）

### 2.2 `history.*`（按需读取历史）

用于在 windowed 模式下读取历史消息（不会把全量塞回 JS）。

- `handle.history.tail({ limit }) -> { startIndex, totalCount, messages, cursor, hasMoreBefore }`
- `handle.history.before(page, { limit }) -> page`
- `handle.history.beforePages(page, { limit, pages }) -> page[]`（减少 IPC 往返，移动端更划算）

`startIndex` 永远是这页第一条消息的**绝对索引**（0-based）。

### 2.3 `locate.findLastMessage()`（后端定位）

用于替代扩展侧对 `getContext().chat` 的“反复倒序扫描”：

```js
const hit = await handle.locate.findLastMessage({
  role: 'assistant',
  hasExtraKeys: ['TavernDB_ACU_IsolatedData'],
  scanLimit: 2000,
});
```

返回：

- `null`：未命中
- `{ index, message }`：命中消息的绝对索引 + 原始消息对象

### 2.4 `metadata.*`（小状态：写 header 的 extensions）

- `handle.metadata.get() -> ChatMetadata`
- `handle.metadata.setExtension({ namespace, value })`
  - 写入 `chat_metadata.extensions[namespace]`
  - `value` 设为 `null` 表示删除（与后端实现一致）

推荐：把“进度/配置/短文本摘要”等小状态落到这里（跨端可迁移，且开销稳定）。

### 2.5 `store.*`（大状态：每 chat 的稳定 KV JSON store）

- `handle.store.getJson({ namespace, key })`
- `handle.store.setJson({ namespace, key, value })`
- `handle.store.deleteJson({ namespace, key })`
- `handle.store.listKeys({ namespace })`

推荐：把“表格/数据库/索引”等大 JSON 状态落到 store，而不是塞进消息体。

### 2.6 `searchMessages()`（Phase 2：纯文本检索，CJK 优化）

```js
const hits = await handle.searchMessages({
  query: '北京烤鸭',
  limit: 20,
  filters: {
    role: 'assistant',
    scanLimit: 5000,
  },
});
```

参数：

- `query: string`：必填，非空
- `limit?: number`：默认 20
- `filters?`：
  - `role?: 'user' | 'assistant' | 'system'`
  - `startIndex?: number` / `endIndex?: number`：限制绝对索引范围
  - `scanLimit?: number`：从尾部向前最多扫描多少条消息（**移动端强烈建议设置**）

返回 `SearchHit[]`：

- `index: number`：绝对索引（0-based）
- `score: number`：命中评分（0~1，越大越匹配）
- `snippet: string`：用于 UI/日志的短片段
- `role: 'user' | 'assistant' | 'system'`
- `text: string`：命中的原始 `mes`

当前实现特性（重要）：

- 不使用向量检索；基于“片段命中评分 + TopK”做召回
- 对 CJK/无空格输入会自动扩展为 bigram tokens（降低“必须全词匹配”的失败率）
- 从 chat 尾部开始按页向前扫描；因此 `scanLimit` 是性能上限开关

