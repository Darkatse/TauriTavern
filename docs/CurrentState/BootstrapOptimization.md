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

