# 从 SillyTavern 扩展适配到 TauriTavern

本指南帮你把现有 SillyTavern 扩展快速适配到 TauriTavern，**解锁 SillyTavern 没有的强大能力**。

> **好消息**：大部分适配工作就是把你以前手写的"找消息、搜消息、存数据"逻辑，替换成一行 API 调用。代码通常会变得更短、更清晰。

## 快速对照表

先看你的代码里有没有这些模式——如果有，TauriTavern 提供了更好的原生替代：

| 你以前的做法 | 问题 | TauriTavern 替代 |
| --- | --- | --- |
| `chat.slice().reverse().find(...)` | 手动遍历，可能在窗口外找不到 | ✅ `handle.locate.findLastMessage()` |
| 手写关键词 `filter/includes` | CJK 不友好，无评分排序 | ✅ `handle.searchMessages()` |
| 数据塞进消息体 + `saveChat()` | 膨胀 payload，数据与消息耦合 | ✅ `handle.store.setJson()` |
| hack 写入 `chat_metadata` | 容量有限，语义不清 | ✅ `handle.metadata.setExtension()` |
| `context.chat.length` 当总楼层数 | 窗口模式下不准确 | ✅ `windowInfo().totalCount` |
| `context.chat.slice(-N)` | 可能拿不到完整历史 | ✅ `handle.history.tail({ limit: N })` |

## 背景：窗口化聊天

TauriTavern 默认采用窗口化加载——前端只保留最近 N 条消息，不会把整个聊天历史塞进 JS 内存。这在长对话和移动端上带来显著的性能优势。

对扩展开发者来说，唯一需要注意的是：**`getContext().chat` 现在只包含窗口内的消息**。但别担心——TauriTavern 提供的 API（`findLastMessage`、`searchMessages`、`history.*`）会透明地穿越窗口边界，在后端扫描完整历史。你只需要把旧代码里的手动遍历换成 API 调用即可。

## 第 1 步：初始化 API

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const api = window.__TAURITAVERN__.api.chat;
const handle = api.current.handle();
```

> 💡 **两端兼容**：用 `if (window.__TAURITAVERN__)` 判断运行环境，让同一个扩展在 SillyTavern 和 TauriTavern 上都能工作。

## 第 2 步：替换"找最后一条消息"

这是最常见的适配场景——几乎所有记忆/数据库类扩展都在做这件事。

**之前**：
```js
const hit = context.chat.slice().reverse().find(
  msg => msg.extra?.MyExtData
);
```

**之后**：
```js
const hit = await handle.locate.findLastMessage({
  role: 'assistant',
  hasExtraKeys: ['MyExtData'],
  scanLimit: 2000,
});
// hit = { index, message } 或 null
```

Rust 后端处理，即使万条消息也毫秒响应。

## 第 3 步：替换关键词搜索

**之前**：
```js
const results = context.chat.filter(msg =>
  msg.mes.includes('关键词')
);
```

**之后**：
```js
const hits = await handle.searchMessages({
  query: '关键词 或 中文短语',
  limit: 20,
  filters: { role: 'assistant', scanLimit: 5000 },
});
```

内置 CJK 优化，自动分词，带匹配评分和摘要片段。

## 第 4 步：迁移数据持久化

这是最令人兴奋的改进——SillyTavern **从未提供** 标准的扩展数据持久化机制。

### 大数据 → `store.*`

**之前**（常见 hack）：
```js
// 把数据塞进最后一条消息...
lastMsg.extra.myData = hugeJsonObject;
saveChat(); // 膨胀聊天文件
```

**之后**：
```js
await handle.store.setJson({
  namespace: 'my-ext',
  key: 'index',
  value: hugeJsonObject,
});
```

独立存储，不影响聊天文件大小。

### 小配置 → `metadata.*`

```js
await handle.metadata.setExtension({
  namespace: 'my-ext',
  value: { lastProcessedFloor: 42, enabled: true },
});
```

存储在 `chat_metadata.extensions[namespace]`，语义清晰。

## 第 5 步：索引语义迁移

如果你的扩展会持久化"已处理到第几楼"之类的进度，注意使用**绝对索引**：

```js
const info = await api.current.windowInfo();
const total = info.totalCount;       // 全量消息数
const absIndex = info.windowStartIndex + windowIndex;  // 窗口索引 → 绝对索引
```

> ⚠️ 持久化时永远存**绝对索引**，不要存窗口索引。

## 第 6 步（可选）：历史遍历

如果你确实需要遍历历史消息，使用分页读取：

```js
let page = await handle.history.tail({ limit: 100 });
while (page.hasMoreBefore) {
  for (const msg of page.messages) { /* 处理消息 */ }
  page = await handle.history.before(page, { limit: 100 });
}
```

移动端推荐批量拉取减少 IPC 调用：

```js
const pages = await handle.history.beforePages(page, { limit: 200, pages: 5 });
```

## 移动端性能建议

TauriTavern 运行在桌面和移动端，几条简单规则让你的扩展在低端设备上也流畅：

- ✅ 用 `findLastMessage` / `searchMessages` 替代 JS 侧遍历
- ✅ 用 `store.*` 存大数据，不塞消息体
- ✅ 总是设置 `scanLimit`，避免全量扫描
- ✅ 用 `beforePages()` 批量分页，减少 IPC 往返
