# Bootstrap Optimization（现状）

本文记录 **已经落地** 的 bootstrap / 启动阶段优化中，与“冷启动内存基线”强相关的部分实现快照。

> 完整的分阶段启动（Shell/Core/Full/APP_READY）链路请看：`docs/CurrentState/StartupOptimization.md`。

## TokenCache（Chat Completions）按聊天分桶

### 解决的问题

计数缓存省掉重新计数：长聊天打开时重新计数要数秒，载入一个桶只要几毫秒。缓存不能整库常驻内存，也不能随聊天历史无限增长。

### 当前实现（契约）

- **按聊天分桶**：每个聊天一个桶 `tokenCache:${版本}:${chatId}`（localforage 实例 `SillyTavern_ChatCompletions`）。键的计法改变时升版本，启动后空闲时删除其他版本的桶。
- **只常驻正在使用的桶**：`CHAT_CHANGED` 换到新聊天的桶，第一次计数前载入。
- **桶只写本次用到的计数**：自载入以来读到或算出的计数。只属于旧消息、旧滑动、旧模型的计数在下一次写入时离开桶，桶的大小以一段会话用到的计数为界。
- **有新计数时写入**：离开聊天、保存聊天、应用转入后台或退出（生命周期 flush `token-cache`）。
- **桶的载入、写入与删除按发出的顺序进行**，桶是最后一次操作留下的样子。
- **随聊天删除**：`CHAT_DELETED` / `GROUP_CHAT_DELETED` 删除聊天的桶；`CHAT_RENAMED` 删除旧名字的桶，聊天在新名字下重新计数。

### 兼容性边界

- 缓存只影响性能：键是模型与内容的哈希，任何桶都不会给出错误的计数。
- 上游导出的 `saveTokenCache()` 写入正在使用的桶。

### 关键代码位置

- `src/scripts/tokenizers.js`：`TokenCacheState`、`readCachedCount` / `rememberCount`、`writeTokenCache`、`removeTokenCache`、`initTokenizers()` 的事件与生命周期挂载

---

## ItemizedPrompts（Prompt Inspector）避免 chat-open whole-load

### 解决的问题

历史实现会在打开聊天时执行 `loadItemizedPrompts(getCurrentChatId())`，并从 IndexedDB 一次性加载 `chatId -> itemizedPrompts[]` 整包数组。

该数组的单条 record 内含大量长字符串（`rawPrompt/finalPrompt/worldInfo/...`），体积通常远大于聊天正文，导致移动端 WebView renderer 常驻集显著膨胀。

### 当前实现（契约）

- **存储形态（Index + Record）**：
  - Index：`tt_prompts_index:${chatId}` → `Array<{ mesId, recordId }>`
  - Record：`tt_prompts_record:${chatId}:${recordId}` → 单条 prompt artifacts（原 record 全量）
- **打开聊天只加载 Index**：`loadItemizedPrompts(chatId)` 仅恢复 `mesId -> recordId` 映射，不加载任何 record。
- **点击才加载 Record**：`.mes_prompt` 点击后，`promptItemize()` 按 `mesId` 懒加载对应 record（以及可选的上一条 record 用于 diff）并渲染 Inspector。
- **生成链路不再常驻 records**：生成结束写入 record + 更新 index，不再把 record `push` 到全局数组里常驻。
- **消息编辑语义保持**：
  - `swapItemizedPrompts()`：仅交换 index 的 `mesId` 映射
  - `deleteItemizedPromptForMessage()`：按上游语义 shift 后续 `mesId`，并删除被删除消息的 record
- **删除/清理覆盖完整**：
  - chat 删除会清理 Index + Records（不留磁盘残留）
  - `clearPrompts` 会清空整个实例

### 兼容性边界

- 仍复用 `localforage` 实例名 `SillyTavern_Prompts`，保证与上游/生态对存储实例名的假设一致。
- legacy 格式（`chatId -> array`）在首次进入该 chat 且 index 不存在时，会被迁移为 Index + Records 并删除旧 key（一次性成本，后续打开聊天不再 whole-load）。

### 关键代码位置

- `src/scripts/itemized-prompts.js`：Index/Record keys、加载/迁移、Inspector 懒加载、swap/delete 语义
- `src/script.js`：生成写入、按钮显示（index presence）
