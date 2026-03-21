# 记忆类扩展 API（当前落地状态）

本文档记录 **已经落地** 的 TauriTavern 记忆类扩展 API 现状，用于后续持续开发与回归。

目标问题：

- windowed payload 下，`getContext().chat` 不再是全量数组，导致上游记忆类扩展契约破裂
- 需要让扩展在不加载全量历史的前提下，仍能：
  - 获取稳定楼层语义（绝对索引）
  - 按需读取历史
  - 后端定位状态
  - 做纯文本召回（不向量，CJK 优化）
  - 可靠持久化扩展状态

## 1. 唯一入口（Public ABI）

唯一入口（刻意不做 alias）：

- `window.__TAURITAVERN__.api.chat`

扩展侧 ready 建议：

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
```

## 2. 已落地能力（Phase 1 + Phase 2）

### 2.1 当前 chat 与楼层语义

- `api.chat.current.windowInfo() -> { mode, totalCount, windowStartIndex, windowLength, chatRef }`
- “window index -> 绝对 index”映射：`abs = windowStartIndex + windowIndex`

### 2.2 历史按需读取

- `handle.history.tail({ limit })`
- `handle.history.before(page, { limit })`
- `handle.history.beforePages(page, { limit, pages })`（批量减少 IPC 往返）

### 2.3 后端定位（从尾部倒序扫描）

- `handle.locate.findLastMessage({ role?, hasTopLevelKeys?, hasExtraKeys?, scanLimit? })`

用于替代扩展侧对 `chat` 的反复倒序扫描（尤其是 st-memory-enhancement / Database_script 这种“找最后状态”逻辑）。

### 2.4 状态持久化（每 chat）

小状态（写入 header）：

- `handle.metadata.get()`
- `handle.metadata.setExtension({ namespace, value })`（`value=null` 表示删除）

大状态（稳定 KV JSON store）：

- `handle.store.getJson({ namespace, key })`
- `handle.store.setJson({ namespace, key, value })`
- `handle.store.deleteJson({ namespace, key })`
- `handle.store.listKeys({ namespace })`

### 2.5 纯文本检索（Phase 2）

- `handle.searchMessages({ query, limit?, filters? }) -> [{ index, score, snippet, role, text }]`

`filters`：

- `role?: 'user' | 'assistant' | 'system'`
- `startIndex?: number` / `endIndex?: number`（绝对索引范围）
- `scanLimit?: number`（从尾部向前最多扫描多少条消息）

当前实现约束（重要）：

- 不上向量检索
- 不做“全量常驻索引库”；直接复用 windowed payload 的 tail/before 分页 I/O，从尾部开始流式扫描
- 对 CJK/无空格 query 自动扩展 bigram tokens，提高召回鲁棒性
- `scanLimit` 是移动端性能上限开关，扩展侧应主动设置

## 3. 持续开发约束（不要回归的点）

- 不要引入新的 alias（坚持唯一入口 `window.__TAURITAVERN__.api.chat`）
- 不要把全量 chat 塞回 JS（除非用户手动关闭 windowed 模式）
- API 报错直接暴露（避免 silent fallback），方便定位扩展适配问题

## 4. 相关文档

- API 参考：`docs/API/Chat.md`
- 适配指南：`docs/API/Migration.md`
- windowed payload 现状：`docs/CurrentState/WindowedPayload.md`

