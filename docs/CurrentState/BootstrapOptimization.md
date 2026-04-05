# Bootstrap Optimization（现状）

本文记录 **已经落地** 的 bootstrap / 启动阶段优化中，与“冷启动内存基线”强相关的部分实现快照。

> 完整的分阶段启动（Shell/Core/Full/APP_READY）链路请看：`docs/CurrentState/StartupOptimization.md`。

## TokenCache（Chat Completions）避免 cold-start whole-load

### 解决的问题

历史实现会在 `initTokenizers()` 中把 IndexedDB 里的 `tokenCache`（整库大对象）一次性读入 JS 内存，导致移动端冷启动常驻集膨胀。

### 当前实现（契约）

- **存储分区**：不再使用整库 key `tokenCache`，改为按 chat 分区持久化 `tokenCache:${chatId}`。
- **启动不加载**：`loadTokenCache()` 只做 legacy key 清理（删除 `tokenCache`），不再读取整库对象。
- **只常驻当前桶**：内存只持有“当前 chat 的一个桶”（`tokenCacheState`），切换 chat 会重置到新桶并按需懒加载。
- **事件语义一致**：
  - `CHAT_CHANGED`：预热当前 chat 桶（懒加载触发点）
  - `CHAT_DELETED` / `GROUP_CHAT_DELETED`：删除对应 `tokenCache:${chatId}`，避免磁盘堆积
- **写回策略**：`saveTokenCache()` 仅 flush 当前 chat 桶（dirty 才写回），不再整库写回。

### 兼容性边界

- cache 是性能优化，不影响 token 计数正确性；升级后旧整库缓存会被清理，短期缓存会重新变冷（可接受的 tradeoff）。

### 关键代码位置

- `src/scripts/tokenizers.js`：`loadTokenCache()` / `saveTokenCache()` / `resetTokenCache()` / `tokenCacheState` / `initTokenizers()` 事件挂载

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
