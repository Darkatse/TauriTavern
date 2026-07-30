# 记忆类扩展 API（当前落地状态）

本文档记录 TauriTavern 记忆、数据库与检索类扩展 API 的当前状态。

TauriTavern 遵循 SillyTavern 1.18.0 契约：`getContext().chat` 是当前聊天的完整、有序消息数组，数组下标就是 0-based 绝对消息索引。增强 API 不替代这一契约，而是把有界读取、定位、搜索和扩展状态持久化交给 Rust 后端。

## 1. 唯一入口

公开 ABI 刻意只有一个入口，不提供 alias：

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const api = window.__TAURITAVERN__.api.chat;
```

## 2. 当前 chat 与身份

- `api.chat.current.ref()`：当前聊天引用。
- `api.chat.current.handle()`：当前聊天 handle。
- `api.chat.current.windowInfo()`：保留六字段 Promise ABI；`mode` 固定为 `'off'`，`windowStartIndex` 固定为 `0`，`windowLength === totalCount === chat.length`。
- `handle.stableId()`：可持久化的稳定聊天身份。
- `handle.summary({ includeMetadata? })`：无需读取消息正文的摘要。

## 3. 有界历史读取

- `handle.history.tail({ limit })`
- `handle.history.before(page, { limit })`
- `handle.history.beforePages(page, { limit, pages })`

这些方法单次只返回请求的页，适合后台批处理或避免扩展再创建一份全量数组。它们不会改变前端 `chat[]` 或 DOM。

分页 cursor 带文件签名；聊天被其他写入改变后，旧 cursor 会明确失败，调用方应重新从 `tail()` 开始。

## 4. 后端定位与搜索

- `handle.locate.findLastMessage({ role?, hasTopLevelKeys?, hasExtraKeys?, scanLimit? })`
- `handle.searchMessages({ query, limit?, filters? })`

`filters` 支持：

- `role?: 'user' | 'assistant' | 'system'`
- `startIndex?: number`
- `endIndex?: number`
- `scanLimit?: number`

当前搜索是轻量文本召回，不引入向量库或全量常驻索引。CJK/无空格 query 会扩展 bigram tokens；扩展应使用 `scanLimit` 明确性能边界。

## 5. 每聊天扩展状态

小状态写入 header 的 namespace：

- `handle.metadata.get()`
- `handle.metadata.setExtension({ namespace, value })`

大状态使用独立 KV JSON store：

- `handle.store.getJson({ namespace, key })`
- `handle.store.setJson({ namespace, key, value })`
- `handle.store.updateJson({ namespace, key, value })`
- `handle.store.renameKey({ namespace, key, newKey })`
- `handle.store.deleteJson({ namespace, key })`
- `handle.store.listKeys({ namespace })`

消息本身的修改仍应通过 `await getContext().saveChat()` 保存；不要 import `script.js` 内部实现，也不要直接写 JSONL。

## 6. 持续开发约束

- 保持 `getContext().chat` 完整、有序，不为内存优化改变数据契约。
- DOM 优化只能发生在渲染层。
- history 分页保持显式、只读，不参与当前聊天状态或 generation。
- API 错误直接传播，不做 silent fallback。
- 不新增公开 alias，不把大扩展状态塞进消息体。

## 7. 相关文档

- API 参考：`docs/API/Chat.md`
- 适配指南：`docs/API/Migration.md`
- Chat payload：`docs/CurrentState/ChatPayload.md`
