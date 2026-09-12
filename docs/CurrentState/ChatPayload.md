# Chat Payload 现状

本文档描述当前聊天 payload 的三个独立机制：前端完整历史契约、完整 payload 原子提交、后端只读分页。三者不得重新耦合成前端数据窗口。

## 1. 核心契约

对合法 SillyTavern JSONL，第一行是 header，后续每个非空记录都是一条消息。当前聊天加载完成后：

- `chat[]` 包含 header 之后的全部消息，顺序与磁盘一致。
- `chat[i]` 始终是 0-based 绝对消息索引。
- generation、扩展、编辑、swipe、删除和保存共享同一个 canonical `chat[]`。
- 任一 JSONL 记录无法解析时，加载整体失败；不得提交部分历史。
- 未显式切换聊天时，角色的 `chat` 文件 stem 在浅层、完整和重复读取之间保持稳定。

这与 SillyTavern 1.18.0 的前端契约一致。TauriTavern 不再提供 `chat_history_mode`，也不存在前端 window state、生成时 backfill 或局部 patch 保存。

## 2. 完整加载与受限 DOM

第一方角色聊天和群聊通过统一的内部 transport 入口加载完整 payload：

- 角色：`loadCharacterChatPayload()`
- 群聊：`loadGroupChatPayload()`

transport 解析完整 JSONL 后直接把同一对象数组交给核心调用方，不再经过本地 Fetch 的 `JSON.stringify()` / `Response.json()` 往返。角色和群聊调用方继续负责 `allowNotFound`、stale-selection guard、header 处理和既有事件时序。`POST /api/chats/get` 与 `POST /api/chats/group/get` 仍是扩展和脚本可主动调用的兼容路由，并复用同一 transport。

角色聊天在完整水合后才绑定本次 payload 请求的 character/chat 快照。若角色当前 stem 不存在、但已有聊天列表非空，则沿用 `replaceCurrentChat()` 的最近聊天语义按需修复并写回；列表为空时才允许创建新聊天。恢复只在打开目标角色时发生，不做启动期全库扫描。

显式打开指定聊天（首页 recent、聊天管理器、书签、分支）统一经由 `selectCharacterById(id, { chatFile })` 与 `openGroupById(id, { chatId })`：不预扫聊天列表、不恢复、不新建；角色 `chat` 与群组 `chat_id` 只在目标加载成功后写回。

所有平台通过共享的 Tauri FileHandle pull stream 有界读取 JSONL。每次加载始终复用同一个文件 handle，并在 EOF、取消或失败时关闭资源；桌面标准模式、portable 模式及自定义数据目录使用同一个已解析 `data_root` runtime scope。

`power_user.chat_truncation` 只限制首次挂载的 DOM 数量，不裁剪 `chat[]`。`Show more messages` 从完整数组中补挂更早楼层，不发起历史 I/O，也不改变数组索引。后续 DOM virtualization 若实施，也只能替换渲染层，不能改变 canonical data contract。

## 3. 完整保存

第一方完整保存通过统一 transport 提交 header 与消息，不再经过本地 Fetch 的请求序列化和解析：

- 角色：`saveCharacterChatPayload()`
- 群聊：`saveGroupChatPayload()`

当前聊天业务入口仍通过 `enqueueChatSave()` 串行调度；扩展调用 `getContext().saveChat()` 也复用该入口。分支、检查点、角色转群和历史重命名复用同一 transport，保留各自的消息范围、metadata 与事件时序。transport 本身不入队，避免队列任务等待自身排队的提交。

commit 在首次异步让出前同步逐记录 `JSON.stringify()`，捕获本次提交私有的 JSON 文本快照。之后的消息或嵌套 metadata 修改不会混入本次保存。快照在任务执行时捕获，不提前为排队任务生成；不深拷贝聊天对象图，也不拼接整份 JSONL 字符串。它仍占用与 payload 大小成正比的临时文本空间，单条记录仍需完整编码，帧预算不是整个保存过程的内存上限。

facade 使用 target-local commit session，按 host 返回的帧预算编码并传输快照，每次只有一帧在途。Android 使用 base64 帧，其他平台使用 raw bytes；finish 阶段校验 ACK 并原子发布。序列化失败不会创建会话；begin 成功后到 finish 之前的失败走 abort，清理失败与原始错误以 `AggregateError` 一并传播。host 的 finish 无论成败都消费会话并清理 stage，因此 finish 之后不再 abort。

`POST /api/chats/save` 与 `POST /api/chats/group/save` 保留为扩展和脚本主动调用的兼容路由，复用同一 transport。成功仍返回 `{ ok: true }`，integrity 冲突仍返回 `400 { error: 'integrity' }`。第一方保存不再产生这些 Fetch 请求，依赖 monkeypatch Fetch 观察保存的扩展不再收到它们；兼容路由不额外加入核心前端保存队列。

host 以 serde 外部标签形状 `{ Variant: payload }` 拒绝，该值本身不是 Error。聊天提交 command 的拒绝在 commit facade 离开 IPC 边界时归一为 Error 并保留原值为 `cause`（其他 command 由 `safeInvoke` 归一）：`{ BadRequest: 'integrity' }` 得到 `code: 'integrity'`，其他对象以其 JSON 文本为 message，字符串原文为 message。这是无损的形状转换，不按错误文案猜测冲突。当前聊天冲突由共享弹窗确认后强制全量保存，拒绝则 reload。不存在保存失败后静默改走另一条写路径的降级逻辑。

完整提交、导入、metadata extension 更新和备份发布共用 storage-core 的 `persist_file`：完成写入与 flush 后，将原写入句柄交给 helper 执行 `sync_all`，关闭后再严格 rename。备份编码器返回原写入句柄，保留到时间戳设置和内容同步完成。分块传输期间不逐块同步；聊天扩展 JSON store 使用同一发布机制，摘要缓存不强制同步。此保证覆盖文件内容同步和运行时原子替换，不包含 rename 后父目录项的断电持久化。

## 4. First-class Tool 消息

Legacy Generate 的工具轮直接保存在同一扁平 `chat[]` 中：

```text
Assistant { mes, tool_calls[] }
Tool      { role: "tool", tool_call_id, mes: result }
```

Assistant 即使没有正文，只要包含 `tool_calls` 就是完整消息。每个 Tool result 拥有真实绝对索引，并通过 `tool_call_id` 归属于之前发出该 call 的 Assistant；关联不依赖物理相邻，因为工具执行期间可能追加图片等副作用消息。

新 writer 固定写入 `is_user:false`、`is_system:true`，使只理解 SillyTavern legacy booleans 的扩展默认过滤 Tool；历史重放只以 `role === "tool"` 为角色事实，不因兼容 booleans 或展示用 `name`/`error` 被编辑而阻塞。Tool 是可见、可编辑、可独立删除的真实楼层，复用 legacy tool floor 的 `smallSysMes` 紧凑样式；展示层按 `tool_call_id` 向前读取最近的 Assistant call，并以旧 formatter 在同一个默认折叠的 `<details>` 中显示 Arguments 与 Result，不复制持久化数据。`chat[]` 物理顺序、DOM `mesid` 与 `.last_mes` 始终表达同一顺序，不再维护“物理尾 Tool / 逻辑尾 Assistant”两套语义。

编辑、删除、移动、复制、隐藏与分支都只处理用户指定的物理消息，不做 owner/result 级联，也不阻止用户制造不完整工具轮。Assistant call 与 Tool result 的配对只在 provider prompt 组装边界执行；只有 provider 无法重放的 missing、orphan、duplicate、无效 ID/参数/结果关系才会带原始 `chat[index]` 明确失败。空 `tool_calls`/legacy invocation 数组视为没有工具事实，非协议必需的展示元数据不会阻断生成。

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
- JSONL 具体 I/O：`tt-adapter-storage-core`。
- Tauri commands：`tauritavern` presentation 层。
- 分页读取实现暂位于 `windowed_payload.rs` 与 `windowed_payload_io.rs`；文件名是内部历史命名，不代表前端 window mode。

## 8. 验证重点

- 长聊天加载后 `chat.length` 等于完整消息数，初始 `.mes` 数量受 `chat_truncation` 限制。
- Show More 只补 DOM，绝对 `mesid` 不变。
- character/group stale load 结果不会覆盖新选择。
- 缺少本地 `chat` 字段的角色在浅层、完整读取和重启后解析为同一个 stem；已有失配聊天按需恢复。
- 完整保存后重开，编辑、删除、swipe、隐藏范围和 metadata 均保持。
- 保存开始后修改消息和嵌套 metadata，不会改变正在传输的快照；后续保存读取新的状态。
- integrity 冲突与其他失败保持区分；非 integrity 的 host 拒绝以可读 message 到达调用方和兼容路由的 `details`；兼容保存路由保持成功和错误响应语义。
- tail/before 对角色和群聊返回相同索引语义，stale cursor 明确失败。
- 旧 settings 中的 `chat_history_mode` 被 serde 作为未知字段忽略，重新序列化时不会保留。
