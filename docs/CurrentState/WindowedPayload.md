# Windowed Payload（分片读写）现状

本文档描述 **当前已经落地** 的 windowed payload 机制：包括 tail 小窗口加载、向前分页（before/before_pages）、以及 windowed save/patch 写入链路；并覆盖 Prompt-backfill（生成时按需回填）与页缓存/批量 IPC 的现状。

目标读者：后续要继续改“聊天记录分片读写/生成上下文”的开发者。

---

## 1. 范围与结论

windowed payload 的第一性原理是把“聊天记录的**常驻内存压力**”从前端移走：

- **UI Window**：只在前端常驻一个小窗口（tail），保证低端 Android 的内存与渲染开销可控。
- **Disk History**：更早消息留在 JSONL 文件里，通过 cursor 进行**分片读取**（before）与**增量写入**（patch/save）。
- **Prompt Window**：生成时（`Generate()`）可短暂加载更多历史，但不写回 `chat`、不渲染、不扩大 UI window。

当前实现已经做到：

1) UI 仍保持小窗口语义不变：进入聊天只加载 tail；“Show more messages” 用户触发才向前分页加载；保存走 windowed patch（优先）或全量保存（回退）。  
2) `Generate()` 链路不再误用 UI window：Tauri + windowed 状态下会做 **JIT Prompt-backfill**，并带 **页缓存 + 批量 IPC** 降低重复读取成本。  
3) 错误不静默：cursor 签名失效会 toast + console 明确提示，并继续用当前 window 生成；其它错误直接抛出。

---

## 2. 数据模型与核心不变量

### 2.1 JSONL Payload 结构

windowed payload 的底层存储是 JSONL 文件：

- 第 1 行：`ChatHeader`（JSON 对象，包含 `chat_metadata` 等信息）
- 第 2 行起：`ChatMessage`（每行一个消息 JSON）

前端 UI 的 `chat` 数组 **不包含 header**，只包含 messages；header 解析后写入 `chat_metadata`。

### 2.2 Cursor（分片读写锚点）

Rust 侧定义：`ChatPayloadCursor { offset, size, modified_millis }`（序列化为 camelCase）。见：

- `src-tauri/src/domain/repositories/chat_repository.rs`

含义：

- `offset`：**当前内存窗口**（UI chat 所对应的文件后缀）在文件中的起始 byte offset（必须是 JSONL 行边界）。
- `size` + `modified_millis`：文件签名。任何外部写入会导致签名变化，从而让旧 cursor 失效。

核心不变量（非常容易被误改）：

> 当 `windowState` 存在时，前端的 `chat` 必须表示“从 `cursor.offset` 开始到 EOF 的一段**连续后缀**消息序列（不含 header）”。

因此：

- before 分页：只会把更早的消息“向前扩展”这段后缀。
- windowed patch/save：只会覆盖/追加这个后缀，不会触碰 `cursor.offset` 之前的历史字节。

---

## 3. 前端：状态、常量与链路

### 3.1 Windowed State（前端契约）

实现位置：

- `src/scripts/tauri/chat/windowed-state.js`

当前窗口状态是一个全局单例（`currentWindow`），在切换聊天时会被覆盖：

- character chat：
  - `{ kind: 'character', characterName, avatarUrl, fileName, cursor, hasMoreBefore, savedMessageCount, dirtyFromIndex }`
- group chat：
  - `{ kind: 'group', id, cursor, hasMoreBefore, savedMessageCount, dirtyFromIndex }`

字段含义（写入链路必须理解）：

- `savedMessageCount`：上次保存时窗口内已落盘的 message 数（对应 `chat.length`）。
- `dirtyFromIndex`：窗口内最早被修改的 message index，用于构建最小 patch。

相关辅助：

- `shiftWindowedMessageSaveState(windowState, deltaMessages)`：在 `showMoreMessages()` 向前插入消息后，把 `savedMessageCount/dirtyFromIndex` 整体平移（索引整体后移）。
- `buildWindowedPayloadPatch(messages, windowState)`：从 `dirtyFromIndex/savedMessageCount` 推导 `ChatPayloadPatchOp`（Append/RewriteFromIndex）。

### 3.2 默认窗口大小（统一常量）

当前默认窗口大小（按运行时区分 desktop/mobile）定义在：

- `src/scripts/tauri/chat/windowed-defaults.js`

并被以下模块共同使用：

- UI tail / Show more 默认值：`src/scripts/tauri/chat/windowed-state.js`
- Prompt-backfill pageSize 默认值：`src/scripts/tauri/chat/prompt-backfill.js`

### 3.3 Transport（Tauri IPC + 规范化）

入口聚合：

- `src/scripts/chat-payload-transport.js`

Tauri transport 实现：

- `src/scripts/tauri/chat/transport.js`

关键点：

- **标准化（必须一致）**
  - `resolveCharacterDirectoryId(characterName, avatarUrl)`：从 avatar URL 推导稳定 character 目录 id（优先用 avatar 内部 id）
  - `normalizeChatFileName(fileName)`：去掉 `.jsonl` 扩展，trim
- JSONL 解析：
  - tail：拼接 `{header + lines}` 再 `jsonlToPayload()`，返回 `payload`（含 header）
  - before：只解析 `lines`，返回 `messages`（不含 header）

### 3.4 UI 读链路：进入聊天与 Show more

character chat 进入聊天（tail）：

- `src/script.js:getChat()`
  - Tauri 模式调用 `loadCharacterChatPayloadTail({ maxLines: DEFAULT_CHAT_WINDOW_LINES })`
  - 解析 header -> `chat_metadata`
  - `chat` 填充为 tail messages
  - `setWindowedChatState(...)` 保存 cursor/hasMoreBefore 与计数器

group chat 进入聊天（tail）：

- `src/scripts/group-chats.js:loadGroupChat()`
  - 调用 `loadGroupChatPayloadTail(...)`
  - 同样设置 windowState（kind = group）

Show more（向前分页）：

- `src/script.js:showMoreMessages()`
  - 调用 `load*ChatPayloadBefore({ cursor, maxLines })`
  - 把返回的 `messages` **prepend** 到 `chat` 与 DOM
  - `shiftWindowedMessageSaveState(windowState, messages.length)` 修正索引计数器
  - 更新 `cursor/hasMoreBefore`

注意：UI 的 showMore 目前仍是 **单页 before**（未接入 before_pages，也未复用 Prompt-backfill 页缓存）。

### 3.5 UI 写链路：windowed patch 保存

character chat 保存：

- `src/script.js:saveChat()`
  - 若 `windowState.kind === 'character'` 且 `windowState.cursor` 存在且 `windowState.fileName === 当前 chat file`：
    - `buildWindowedPayloadPatch(trimmedChat, windowState)` 推导最小 `patch`
    - 调用 `patchCharacterChatPayloadWindowed({ cursor, header, patch })`
    - 成功后用返回的新 cursor 更新 `windowState`，并更新 `savedMessageCount/dirtyFromIndex`
  - 否则回退到全量保存：`saveCharacterChatPayload({ payload })`（临时文件 + `save_chat_payload_from_file`）
  - integrity 错误会弹窗要求用户输入 `OVERWRITE` 决定是否强制覆盖；其它错误直接 toast/console（不静默）

group chat 保存：

- `src/scripts/group-chats.js:saveGroupChat()`
  - 与 character chat 同构：优先 `patchGroupChatPayloadWindowed(...)`，否则全量 `saveGroupChatPayload(...)`

---

## 4. 生成链路：Prompt-backfill（JIT 回填 + 页缓存 + 批量 IPC）

### 4.1 Generate() 如何消费回填后的历史

位置：

- `src/script.js`（`Generate()` 内部）
- `src/scripts/tauri/chat/prompt-backfill.js`

行为：

1) `Generate()` 先确定 `this_max_context`（token budget）
2) 若 Tauri + `windowState.cursor && windowState.hasMoreBefore`：
   - 调用 `buildGenerationChatWithBackfill({ baseMessages: chat, windowState, contextBudgetTokens: this_max_context })`
   - 得到 `generationChat`（仅本次生成使用）
3) 后续 prompt 组装、拦截器、WI 等全部基于 `generationChat`，而不是 UI `chat`

关键约束：

- 回填不会修改 UI `chat`、不会更新 `windowState.cursor`、不会触发 DOM 改动。
- 回填失败时：
  - 若是 cursor 相关错误：toast + console.error，然后继续用当前 `chat` 生成
  - 其它错误：直接抛出（不静默降级）

### 4.2 页缓存（仅 Prompt-backfill）

实现：

- `src/scripts/tauri/chat/prompt-backfill.js` 内部 `beforePageCache`（Map + LRU）

缓存 key 构成：

- chat id：使用 transport 的标准化（character: `resolveCharacterDirectoryId + normalizeChatFileName`；group: `normalizeChatFileName(id)`）
- cursor signature：`offset:size:modifiedMillis`
- `maxLines`：页大小

策略：

- 容量：mobile 8 页 / desktop 12 页
- LRU：每次 get 会把条目移动到 Map 尾部；超限逐出最早插入项

### 4.3 批量 IPC（before_pages）

为减少多次 IPC 往返，Prompt-backfill 在 cache miss 时会调用批量接口预取多页：

- 前端 transport：
  - `loadCharacterChatPayloadBeforePages(...)`
  - `loadGroupChatPayloadBeforePages(...)`
- 后端 Tauri commands：
  - `get_chat_payload_before_pages`
  - `get_group_chat_payload_before_pages`

当前实现方式是 **应用层循环**：`ChatService::*_before_pages_lines()` 内部重复调用 repository 的单页 before（一次 IPC 返回多页）。这保持了仓储契约稳定，但仍减少了 IPC 往返次数。

---

## 5. 后端：commands / service / repository 分层与读写语义

### 5.1 Tauri Commands（Presentation）

实现位置：

- `src-tauri/src/presentation/commands/chat_commands.rs`

读取：

- `get_chat_payload_tail(character_name, file_name, max_lines, allow_not_found?) -> ChatPayloadTail`
- `get_chat_payload_before(character_name, file_name, cursor, max_lines) -> ChatPayloadChunk`
- `get_chat_payload_before_pages(character_name, file_name, cursor, max_lines, max_pages) -> Vec<ChatPayloadChunk>`
- group 对应：
  - `get_group_chat_payload_tail(id, ...)`
  - `get_group_chat_payload_before(id, ...)`
  - `get_group_chat_payload_before_pages(id, ...)`

写入：

- `save_chat_payload_windowed(dto) -> ChatPayloadCursor`
- `patch_chat_payload_windowed(dto) -> ChatPayloadCursor`
- `save_chat_payload_from_file(dto) -> ()`（全量保存回退路径）
- group 对应：
  - `save_group_chat_payload_windowed(dto)`
  - `patch_group_chat_payload_windowed(dto)`
  - `save_group_chat_from_file(dto)`

### 5.2 应用层（Application）

实现位置：

- `src-tauri/src/application/services/chat_service.rs`

职责：

- 维持参数校验（例如 `max_lines/max_pages > 0`）
- before_pages 仅做循环聚合（不在这里引入缓存与复杂策略）

### 5.3 仓储层（Infrastructure）：FileChatRepository

实现位置：

- 读取：`src-tauri/src/infrastructure/repositories/file_chat_repository/windowed_payload.rs`
- 写入 patch：`src-tauri/src/infrastructure/repositories/file_chat_repository/windowed_patch.rs`
- cursor/IO 辅助：`src-tauri/src/infrastructure/repositories/file_chat_repository/windowed_payload_io.rs`

读语义要点：

- tail：
  - 返回 `header`（第一行）与最后 `max_lines` 条 message 行
  - `cursor.offset` 指向“本次返回的第一条 message 行”的 byte offset（若无消息则为 header_end_offset）
  - `has_more_before = cursor.offset > header_end_offset`
- before：
  - **严格校验 cursor 签名**（`size + modified_millis`）
  - 校验 `cursor.offset` 不越界、且不在 header body 之前
  - 读取 `cursor.offset` 之前最多 `max_lines` 条 message 行，并返回新的 cursor

写语义要点：

- patch/save 都会：
  - 获取写锁（同一 payload 序列化写入）
  - 校验 cursor 签名
  - 校验 integrity（除非 `force`；注意：`force` 只跳过 integrity，不跳过 cursor 校验）
  - 写入后更新 cursor（新的 size/modified_millis）
  - 清理相关缓存并写入备份（backup）
- `save_*_payload_windowed`：
  - 保留 `cursor.offset` 之前的原始字节，截断并用传入的 JSONL `lines` 覆盖/续写窗口后缀
- `patch_*_payload_windowed`：
  - `Append`：追加 lines 到 EOF
  - `RewriteFromIndex(startIndex)`：从 `cursor.offset` 起第 `startIndex` 条 message 行开始截断并重写
  - `startIndex` 是 **相对 cursor.offset 的 0-based 行号**（因此要求前端 `chat` 与 cursor 始终对应连续后缀）

---

## 6. 当前支持/不支持边界

支持：

- character/group chat 都支持 tail/before/patch 的 windowed payload 语义
- UI 小窗口 + 手动 showMore 的分页加载
- 生成时 JIT Prompt-backfill（不改 UI window）
- Prompt-backfill 的页缓存 + 批量 IPC（before_pages）

明确不承诺：

- “进入聊天就预加载到 prompt window”（会提高常驻内存；当前设计刻意不做）
- UI showMore 自动接入批量接口/页缓存（当前只保证 prompt-backfill 侧收益）
- `force` 覆盖 cursor 失效（目前不支持；cursor 失效必须 reload/resync）

---

## 7. 持续开发约束（最容易误改的契约）

1) `chat` 必须始终表示 `cursor.offset` -> EOF 的连续后缀（不含 header）。  
2) 所有会修改 `chat` 的操作都必须正确更新 dirty 语义（例如 `markWindowedChatDirtyFromIndex(...)`），否则 patch 可能覆盖错误范围。  
3) cursor 校验失败不是“可忽略的小错误”，它意味着窗口与文件不一致；当前策略是**提示并继续生成（仅 Prompt-backfill）**，但写入链路不应静默吞掉。  
4) `DEFAULT_CHAT_WINDOW_LINES_*` 是 windowed payload 的性能杠杆，修改必须通过 `windowed-defaults.js` 统一，避免 UI/Prompt-backfill 脱节。  
5) cache key 必须使用 transport 标准化后的稳定 id（避免 avatarUrl 表现差异导致命中率下降）。

---

## 8. 最小回归面（改动此链路后必须过一遍）

- 进入 character chat：只加载 tail，滚动/渲染正常，windowState 被正确设置
- 进入 group chat：同上
- Show more：能向前分页插入消息；cursor/hasMoreBefore 正常推进；保存后不丢消息
- 保存：正常 patch 保存；触发 integrity 错误时弹窗逻辑正确；cursor 更新后继续可保存
- 生成：在长聊天中 AI 能引用更早内容；cursor 失效时 toast 提示且仍可继续生成（但不会静默）
