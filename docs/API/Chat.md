# `window.__TAURITAVERN__.api.chat` — API 参考

TauriTavern 为“记忆/数据库/检索类扩展”扩展开发者提供的增强 API。

> **核心理念**：把 SillyTavern 扩展开发中最常见的脏活累活——找消息、搜内容、存数据——变成一行 API 调用。Rust 后端处理重活，JS 侧只管调接口。

## 0. 快速上手

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const api = window.__TAURITAVERN__.api.chat;
const handle = api.current.handle();
```

> TauriTavern 遵循 SillyTavern 的完整历史契约：`getContext().chat` 是当前聊天的完整、有序消息数组。下面的增强 API 适合在 Rust 后端执行有界读取、定位、检索与独立状态持久化，避免扩展重复实现重扫描或复制大数据。

## 1. 核心入口

启用[历史滑动按需加载](../CurrentState/ChatPayload.md#21-历史滑动按需加载)时，`getContext().chat` 的历史候选槽位可能为 null；本页后端读取 API 仍返回完整内容。

| 方法 | 返回值 | 说明 |
| --- | --- | --- |
| `api.chat.open(ref)` | `ChatHandle` | 打开指定聊天 |
| `api.chat.current.ref()` | `ChatRef` | 当前聊天的引用 |
| `api.chat.current.handle()` | `ChatHandle` | 当前聊天的操作句柄 |
| `api.chat.current.windowInfo()` | `Promise<WindowInfo>` | 当前完整历史的兼容状态信息 |

**`ChatRef` 类型**：

- 角色聊天：`{ kind: 'character', characterId, fileName }`
- 群聊：`{ kind: 'group', chatId }`

**`WindowInfo` 类型**：

| 字段 | 说明 |
| --- | --- |
| `mode` | 固定为 `'off'` |
| `chatKind` | `'character' \| 'group'` |
| `chatRef` | 当前 `ChatRef` |
| `totalCount` | 聊天总消息数（不含 header） |
| `windowStartIndex` | 固定为 `0` |
| `windowLength` | 完整消息数，与 `totalCount` 相同 |

`windowInfo()` 保留 Promise 与字段形状是为了稳定公开 ABI；它不触发后端 summary 查询，也不对应可配置加载模式。消息数组索引就是绝对索引：

```js
const info = await api.current.windowInfo();
const absIndex = info.windowStartIndex + chatIndex; // 等同于 chatIndex
```

## 2. ChatHandle 能力

通过 `api.open(ref)` 或 `api.current.handle()` 获取 `ChatHandle`。

---

### `locate.findLastMessage()` — 精准定位，不再手动遍历

> 💡 **解决的痛点**：定位最后一条包含特定数据的消息通常需要 `chat.slice().reverse().find(...)` 手动遍历；后端定位可以限定扫描范围，避免扩展重复复制和扫描长数组。

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
| `role` | `'user' \| 'assistant' \| 'system' \| 'tool'` |
| `text` | 命中消息的完整 `mes` 文本 |

**技术特点**：
- 基于片段命中评分 + TopK 召回（非向量检索，轻量高效）
- CJK / 无空格文本自动 bigram 分词，大幅提升中日韩文匹配率
- 纯标点或符号 query 中的连续片段按原始字符序列做字面量子串匹配；混合 query 继续使用既有分词评分语义
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

数据存储在 `chat_metadata.extensions[namespace]` 中，与聊天文件一同迁移。每次调用只修改指定 namespace，不自动合并到当前页面的活 metadata。

---

### `summary()` / `stableId()`

```js
// 获取聊天摘要（不需要加载完整消息）
const summary = await handle.summary({ includeMetadata: true });

// 获取可持久化的稳定 ID
const id = await handle.stableId();
```

`summary()` 默认只返回轻量摘要。`includeMetadata: true` 返回完整 metadata，读取成本随返回数据大小增长。

---

### `history.*` — 按需分页读取

扩展希望以有界页面处理历史、避免再创建一份全量数组时使用：

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

## 3. 保存进度事件（window）

聊天写盘有两个层次的 window 事件。两者都是纯广播，监听者抛错不会影响保存本身。

### `tt-record-save` — 队列层

```js
window.addEventListener('tt-record-save', event => {
  // event.detail.pending: 尚未排空的聊天记录写盘任务数（含排队）
});
```

入队即计、完成即减，因此包含排队与压缩等待。它回答的是**「现在能不能杀后台」**：`pending === 0` 表示队列已排空。只统计聊天记录文件写入，不含 IndexedDB post-save、设置与角色卡保存。

### `tt-chat-commit` — 单次写盘的字节进度

```js
window.addEventListener('tt-chat-commit', event => {
  // event.detail.pending: 正在进行的提交数
  // event.detail.primary: 最早开始的提交（移动端单行显示用），无提交时为 null
  // event.detail.commits: 全部进行中的提交
});
```

每次提交的形状：

| 字段 | 说明 |
| --- | --- |
| `id` | 单调递增的提交序号 |
| `kind` | `payload` / `metadata` / `metadataExtension` |
| `targetKind` | `character` 或 `group` |
| `status` | `writing`（写分块）或 `finishing`（已写满，宿主正在发布） |
| `written` / `total` | 已写入字节数 / 本次提交的总字节数 |
| `percent` | `written / total`，0–100 的整数 |
| `startedAt` | `Date.now()` 起点，用于自算耗时 |

事件只携带事实，不携带展示文案；标签由消费者按 `kind` 决定。TauriTavern 自带的指示器使用这组短名：`payload` → `savechat`，`metadata` / `metadataExtension` → `savemeta`。

覆盖范围是全部聊天写盘：单人/群聊 × 聊天内容/聊天元数据/元数据扩展。总量在前端按 UTF-8 精确测量（约 900MB/s，94MB 聊天约 100ms），分块大小由宿主决定（Android 256KB、iOS 1MB、桌面 4MB），所以移动端大聊天的进度粒度约 0.3%。

`status` 转为 `finishing` 后 `percent` 已经是 100，但宿主还在做 rename / fsync / 缓存失效，这段时间通常很短但并非零。需要「进度到顶后保持一小段收尾态」的界面可以直接用这个状态。

### TauriTavern 自带的指示器

TauriTavern 已经用这个事件在输入框 placeholder 上显示 `savechat:53%` 这类单行进度（`src/scripts/tauri/save-progress-indicator.js`，由 `bootstrap.js` 在宿主就绪后安装），只在输入框为空时可见，队列排空后自动还原。

扩展如果也想渲染保存进度，请先检查能力标记，避免两处同时改写 placeholder：

```js
if (document.documentElement.dataset.ttSaveProgress === 'on') {
  // TauriTavern 已经显示，扩展不要重复实现
}
```

