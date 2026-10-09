# Chat Payload 现状

本文档描述前端完整历史契约、完整 payload 与 metadata 原子提交、后端只读分页。这些机制不得重新耦合成前端数据窗口。

## 1. 核心契约

聊天 JSONL 的首个非空记录是 header，后续每个非空记录都是一条消息。当前聊天加载完成后：

- `chat[]` 包含 header 之后的全部消息，顺序与磁盘一致。
- `chat[i]` 始终是 0-based 绝对消息索引。
- generation、扩展、编辑、swipe、删除和保存共享同一个 canonical `chat[]`。
- 完整替换聊天内容通过 `replaceChatContents()` 逐项写入消息引用并调整长度，保持 `chat[]` 实例稳定，不把历史展开成函数参数。这是为了保证 Android 上的 V8 引擎不会在超过 65536 条时抛出 RangeError。
- 任一 JSONL 记录无法解析时，加载整体失败；不得提交部分历史。
- 未显式切换聊天时，角色的 `chat` 文件 stem 在浅层、完整和重复读取之间保持稳定。

消息集合与索引遵循 SillyTavern 的完整历史契约；显式开启历史滑动按需加载时，候选内容采用下述受限表示。TauriTavern 不再提供 `chat_history_mode`，也不存在前端 window state、生成时 backfill 或局部 patch 保存。

### 1.1 统一格式底线

- header 和消息均须为 JSON object。`chat_metadata.integrity` 可缺省，出现则须为非空字符串，保留空白并按原值比较；第一方仍生成 UUID，其他字段由使用它们的用例解释。
- header 中的 `chat_metadata` 及其内部同名字段取最后一次出现的值；integrity 校验基于最终生效的 metadata。
- 严格解码 UTF-8，结构空白仅限空格、Tab、CR、LF。首个记录前允许空白和最多一个 BOM，由格式层统一消费；正文记录外不允许 BOM。
- 无记录可读为空聊天；新完整提交必须自带合法 header，force 也不例外。无消息聊天写为 header-only。
- 读取严格验证实际解释的记录，不跳过坏记录；原生完整字节提交只验证 header，不额外扫描正文。
- TT 重新序列化写出无 BOM 的 UTF-8；原样传输、备份和复制保留字节。metadata 更新保留正文原字节，见 §3.1。
- byte offset 以原文件计数；空白行不占逻辑记录编号。分页和 cold 消息切片不另设 header/BOM 前导区。

### 1.2 目录查询投影

目录信息由 storage-core 聊天仓储解释，userdata 只映射结果；各类查询保留所需的读取范围。

- header 合法时，损坏的末条预览显示“Preview unavailable”，保留条目和消息记录位置。正文缺省、空正文及 header-only 不属于损坏。
- 日期使用末条有效 `send_date`，否则使用 mtime；正文不可用不抹去有效日期。
- 角色列表返回完整末条正文，摘要只缓存短预览。搜索发现不可用投影时不缓存整查询，即使该文件未命中。
- 普通摘要、搜索结果和备份详情使用轻量投影，完整 `chat_metadata` 仅在显式请求时读取并附加到本次响应。
- 目录索引是可重建缓存，版本不匹配或文件损坏时按需重建。旧索引在首次加载时清理；清理或持久化失败记录告警，不阻塞目录查询。
- 删除聊天或角色前从当前 header 查询身份。合法旧聊天缺少 integrity 表示无身份；读取或格式错误会停止相关删除操作。目录展示按文件报告并跳过失败项。

目录搜索匹配原始 JSONL。目录可见不保证全文可加载；I/O、编码、解压及必需 header 错误继续传播。

普通摘要缓存的占用随条目及摘要字段增长，不随未知 metadata 体积增长。显式完整 metadata 响应、完整末条正文和搜索的整行读取有各自的数据规模成本；此处不承诺整个聊天链路恒定内存。

## 2. 完整加载与受限 DOM

第一方角色聊天和群聊通过统一的内部 transport 入口加载完整 payload：

- 角色：`loadCharacterChatPayload()`
- 群聊：`loadGroupChatPayload()`

transport 解析完整 JSONL 后直接把同一对象数组交给核心调用方。角色和群聊调用方负责 `allowNotFound`、stale-selection guard、header 处理和既有事件时序。

扩展的兼容 get 共用仓储读取器，将 JSONL 按需转换为 JSON 数组字节，由消费者解析；接口与流式错误语义见 [FrontendHostContract §4.3](../FrontendHostContract.md#43-路由表public)。

角色聊天在完整水合后才绑定本次 payload 请求的 character/chat 快照。若角色当前 stem 不存在、但已有聊天列表非空，则由 `resolvePersistedChat` 按最近聊天语义按需修复并写回；列表为空时才允许创建新聊天。恢复只在打开目标角色时发生，不做启动期全库扫描。

显式打开指定聊天（首页 recent、聊天管理器、书签、分支）统一经由 `selectCharacterById(id, { chatFile })` 与 `openGroupById(id, { chatId })`：不预扫聊天列表、不恢复、不新建；角色 `chat` 与群组 `chat_id` 只在目标加载成功后写回。

第一方默认加载通过共享的 Tauri FileHandle 流读取 JSONL，以同一已打开文件及其初始大小为界；提前 EOF 视为失败，完成、取消或失败时关闭资源。各运行模式共用已解析的 `data_root` scope。

`power_user.chat_truncation` 只限制首次挂载的 DOM 数量，不裁剪 `chat[]`。`Show more messages` 从完整数组中补挂更早楼层，不发起历史 I/O，也不改变数组索引。后续 DOM virtualization 若实施，也只能替换渲染层，不能改变 canonical data contract。

### 2.1 历史滑动按需加载

`cold_swipes_enabled` 默认关闭，重载生效，与 DOM 虚拟化独立。启用后，仅当前聊天采用冷表示；兼容 get、导出和落盘 JSONL 保持完整。

- 加载时保留全部楼层、当前正文、变量、`swipe_id` 和数组长度；符合条件的历史消息将非当前滑动槽位置为 null，末楼完整。后续按需读回，不自动冷藏。
- `tt_swipe_cold: { sourceId, record }` 引用本次加载的源记录：`record` 按非空记录从 0 编号，header 为 0，不随消息重排改变。源文件由当前聊天持有，路径替换不改变来源；切换聊天或重载时释放。
- null 表示未加载，非 null 值及追加槽位以运行时数据为准；读回与保存按槽位补齐，保留运行时 `swipe_id`。读取其他滑动或删除槽位前需读回；未读回就缩短数组会被拒绝。保存时移除标记并原子发布。

投影与合并位于 storage-core 的 `cold_swipes.rs`，前端通过 `chat-payload-transport.js` 接入，Tauri 资源寿命由 host 管理。

## 3. 统一聊天提交

第一方聊天保存经由 `chat-payload-transport.js`；当前聊天由 `enqueueChatSave()` 串行调度，transport 不重复入队。兼容路由与事件契约见 [FrontendHostContract §4.3](../FrontendHostContract.md#43-路由表public)。

提交在首次异步让出前捕获 JSON 文本快照，当前聊天在队列任务开始时捕获；后续修改不混入本次保存。分帧限制传输开销，快照仍随内容规模增长。

聊天与世界书共用 storage-core 的提交会话、帧上限和暂存生命周期，各仓储负责内容校验与发布。host 确认数据完整后才发布；平台传输限制见 [AndroidDevelopment §11](../AndroidDevelopment.md#11-android-大型-byte-ingress)。

提交和取消错误向调用方传播；收尾及启动清理失败只记告警，不改写提交结果。integrity 冲突按明确错误码识别。

聊天发布先同步暂存文件，再原子替换目标；失败不回退为覆盖复制。此保证不包含 rename 后父目录项的断电持久化。

### 3.1 Metadata 保存

`saveMetadata()` / `getContext().saveMetadata()` 替换 JSONL header 内的整个 `chat_metadata`，字段删除也会落盘；header 其他字段保留语义，正文逐字节保留。与 SillyTavern 1.18.0 不同，它不顺带保存消息，修改消息的扩展必须显式调用完整保存。

当前聊天的 metadata 与完整保存共用 `persistedChatMetadata()` 和保存队列。该 helper 排除 `lastInContextMessageId`，不修改活 metadata；metadata 保存不遍历或传输消息、不保存消息派生缓存，也不取消待执行的完整保存。integrity 弹窗及恢复留在同一次队列任务内：确认后强制完整保存，拒绝则 reload；缺文件或普通错误不回退。

metadata 操作要求目标文件存在。新群聊在首次问候扩展事件前绑定 metadata 和 identity、发布初始 header，后续初始化不得覆盖事件修改。扩展的 [`metadata.setExtension()`](../API/Chat.md) 仅修改磁盘目标的指定 namespace，不加入当前聊天队列或合并活 metadata。

storage-core 的 `chat_metadata.rs` 在路径 mutation lock 内读取最新 header、应用修改并复制正文，复用统一发布机制；成功后失效缓存并通知备份协调器。内容签名必须对应完整发布文件，不能用上传的 metadata 字节代替。integrity 表示身份而非内容版本，进程内锁不提供跨进程或 Sync 冲突隔离。

JS 与 IPC 成本随提交的 metadata 大小增长；Rust 工作内存随 header 与新值增长，不随正文增长。磁盘仍需读写完整替代文件，成本为 Θ(文件大小)，文件 mtime 随之更新。header 不保证字段顺序或格式，正文保持原字节。

### 3.2 删除与重命名

删除或重命名打开中的聊天前先经 `prepareCurrentChatFileChange()`：生成中拒绝，否则等待一次完整保存（`saveChatConditional()`，同时取消已排定的防抖保存），保存失败则中止操作。`doNewChat({ deleteCurrentChat: true })` 在清空前使用同一准入。

文件操作在开始时确定目标（头像或群组 id 加文件名），角色记录由 `updateRemoteChatName(characterId, name)` 写入。上游导出 `replaceCurrentChat`、`deleteCharacterChatByName`、`deleteGroupChatByName` 与界面共用同一套删除与替换实现。删除打开中的聊天后，按“清空 → 重置元数据 → 记录指向替代聊天 → 加载 → 保存记录”替换；改名后由 `renameGroupOrCharacterChat` 让仍指向旧名的记录跟随。文件操作成功后，后续步骤失败报告“已删除 / 已改名，但……”。导出只在目标是打开中的聊天时先保存。

按名删除也会加载替代聊天。已确认的扩展边界（2026-10-09）：

- Discordia 0.7.3 删除后再次清空宿主聊天的流程不支持；它可能清空并在后续保存时覆盖替代聊天。
- Folder Manager 3.0.15 批量删除最后一条当前聊天后会自行新建，可能多留一份新聊天；此差异已接受。

已知边界：当前聊天没有显式会话；准入不阻止之后新发起的保存，也不覆盖队列外的批量写入（`renamePastChats`、`renameGroupMember`）。

## 4. First-class Tool 消息

Legacy Generate 的工具轮直接保存在同一扁平 `chat[]` 中：

```text
Assistant { mes, tool_calls[] }
Tool      { role: "tool", tool_call_id, mes: result }
```

Assistant 即使没有正文，只要包含 `tool_calls` 就是完整消息。每个 Tool result 拥有真实绝对索引，并通过 `tool_call_id` 归属于之前发出该 call 的 Assistant；关联不依赖物理相邻，因为工具执行期间可能追加图片等副作用消息。

新 writer 固定写入 `is_user:false`、`is_system:true`，使只理解 SillyTavern legacy booleans 的扩展默认过滤 Tool；历史重放只以 `role === "tool"` 为角色事实，不因兼容 booleans 或展示用 `name`/`error` 被编辑而阻塞。Tool 是可见、可编辑、可独立删除的真实楼层，复用 legacy tool floor 的 `smallSysMes` 紧凑样式；展示层按 `tool_call_id` 向前读取最近的 Assistant call，并以旧 formatter 在同一个默认折叠的 `<details>` 中显示 Arguments 与 Result，不复制持久化数据。`chat[]` 物理顺序、DOM `mesid` 与 `.last_mes` 始终表达同一顺序，不再维护“物理尾 Tool / 逻辑尾 Assistant”两套语义。

编辑、删除、移动、复制、隐藏与分支都只处理用户指定的物理消息，不做 owner/result 级联，也不阻止用户制造不完整工具轮。Assistant call 与 Tool result 的配对只在 provider prompt 组装边界执行；只有 provider 无法重放的 missing、orphan、duplicate、无效 ID/参数/结果关系才会带原始 `chat[index]` 明确失败。空 `tool_calls`/legacy invocation 数组视为没有工具事实，非协议必需的展示元数据不会阻断生成。

SillyTavern 1.19.0 新增的工具级联删除及其参数不在 TT 支持范围内。

Tool call 不进入 `swipe_info`；owner Assistant 只保留 `saveReply` 原本创建的普通单 swipe 元数据，核心 UI 不再为工具轮维护可切换状态。Tool 本身不可 swipe。若物理尾是 Tool，append/continue/swipe 的生成结果作为新的 Assistant 楼层保存，不覆盖 Tool，也不寻找所谓“逻辑 Assistant 尾”；用户可以保留、编辑或删除这次结果。

calls 与全部 results 只在工具执行完成后一次性提交，避免工具 action 保存半成品 transcript。Legacy local 与 MCP tools 共用这一 writer；MCP `OutcomeUnknown` 终止当前批次且不伪造或部分提交结果。新 writer 不写 `extra.tool_invocations`；该字段仅用于读取旧 synthetic tool floors。

Chat Completion preset 的“剥离旧函数调用结果”只改变 provider prompt：开启后，最后一条用户消息之前的完整工具轮（包括发起调用的 Assistant 消息及全部 Tool results）都会省略，只保留不含工具调用的普通 Assistant 消息。该投影在 prompt 正则、reasoning 正则、generation interceptor 与 World Info 扫描前完成，因此已省略的楼层不参与深度或激活计算；provider 组装边界会用同一投影再次校验 interceptor 可能修改的历史。当前用户消息之后的递归工具链仍完整重放。该选项不修改 `chat[]`、DOM 或持久化历史，也不作用于 Agent prompt assembly。

工具执行中的即时反馈只属于前端运行态：整批 calls 校验通过后，UI 在 owner Assistant 上显示 pending cards，并随每个工具完成更新结果；所有工具结束后，pending 消失，持久化 Tool 楼层按自身物理位置显示。pending 状态不进入 `chat[]`、不保存、也不提前发出工具事件，但会在 ChatSurface 重挂载时按同一 Assistant 对象恢复。

纯 tool-only Assistant 仍保留 owner 楼层并持续更新流式/pending UI，但不发出 legacy `MESSAGE_RECEIVED` / `CHARACTER_MESSAGE_RENDERED`；完整 calls/results 原子提交后只发出一次 `TOOL_CALLS_PERFORMED` / `TOOL_CALLS_RENDERED`，递归产生的最终可见 Assistant 再按普通消息语义发出角色事件。包含正文、reasoning 或 media 的 Assistant 不属于 tool-only，继续保持既有角色事件语义。

## 5. 独立只读分页

Rust 仍保留 JSONL tail/before 读取，因为 Agent 和扩展可能只需要一个有界历史切片：

- `get_chat_payload_tail` / `get_group_chat_payload_tail`
- `get_chat_payload_before` / `get_group_chat_payload_before`
- `get_chat_payload_before_pages` / `get_group_chat_payload_before_pages`

分页 cursor 包含 offset、文件大小和修改时间签名。`before` 必须验证签名；文件已变化时返回明确错误，调用方应重新从 tail 建立读取会话。

分页是显式查询能力，不参与当前聊天的 `chat[]`、DOM、generation 或保存。`window.__TAURITAVERN__.api.chat` 的 `history.tail/before/beforePages` 是其公开前端入口。

聊天摘要、最近记录和搜索属于可重建投影：单个文件失败只排除该文件并报告错误，持久化摘要索引写回失败只记录告警。真实目标 JSONL 的完整加载与保存仍保持整体失败语义。

## 6. `windowInfo()` ABI

`api.chat.current.windowInfo()` 保留既有六字段 Promise ABI，但现在只描述完整历史：

```js
{
  mode: 'off',
  chatKind,
  chatRef,
  totalCount: chat.length,
  windowStartIndex: 0,
  windowLength: chat.length,
}
```

`mode: 'off'` 是公开 API 的稳定值，不是可配置模式，也不会触发后端 summary 查询。

## 7. 代码边界

前端：

- `src/script.js`：角色聊天 canonical load/save、DOM truncation、Show More、保存队列。
- `src/scripts/group-chats.js`：群聊 canonical load/save。
- `src/scripts/chat-payload-transport.js`：完整 payload transport 公共入口。
- `src/scripts/tauri/chat/transport.js`：完整 payload Tauri transport 与共享 FileHandle pull stream 接入边界。
- `src/scripts/tauri/chat/jsonl.js`：同步 JSON 记录快照、字节分帧与 JSONL 读取。
- `src/scripts/tauri/chat/commit.js`：快照捕获时机、IPC 会话、ACK 校验与提交错误分类。
- `src/tauri/main/services/files/readable-file-stream-service.js`：跨平台 plugin-fs open/read/close pull stream。
- `src/tauri/main/api/chat.js`：扩展历史分页 API 与 `windowInfo()`。

Rust：

- DTO / service：`tt-application`。
- repository ports：`tt-ports`。
- JSONL 格式与具体 I/O：`tt-adapter-storage-core`，共同格式规则位于 `chat_jsonl.rs`。
- Tauri commands：`tauritavern` presentation 层。
- 分页读取实现暂位于 `windowed_payload.rs` 与 `windowed_payload_io.rs`；文件名是内部历史命名，不代表前端 window mode。

## 8. 验证重点

- 长聊天加载后 `chat.length` 等于完整消息数，初始 `.mes` 数量受 `chat_truncation` 限制。
- Show More 只补 DOM，绝对 `mesid` 不变。
- character/group stale load 结果不会覆盖新选择。
- 缺少本地 `chat` 字段的角色在浅层、完整读取和重启后解析为同一个 stem；已有失配聊天按需恢复。
- 完整保存后重开，编辑、删除、swipe、隐藏范围和 metadata 均保持。
- metadata 保存后 header 字段更新且正文逐字节不变；待保存的消息删除不被取消，新群聊的问候事件可立即保存 metadata。
- 保存开始后修改消息和嵌套 metadata，不会改变正在传输的快照；后续保存读取新的状态。
- integrity 冲突与其他失败保持区分；非 integrity 的 host 拒绝以可读 message 到达调用方和兼容路由的 `details`；兼容保存路由保持成功和错误响应语义。
- tail/before 对角色和群聊返回相同索引语义，stale cursor 明确失败。
- 旧 settings 中的 `chat_history_mode` 被 serde 作为未知字段忽略，重新序列化时不会保留。
