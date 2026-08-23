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

所有平台通过共享的 Tauri FileHandle pull stream 有界读取 JSONL。每次加载始终复用同一个文件 handle，并在 EOF、取消或失败时关闭资源；桌面标准模式、portable 模式及自定义数据目录使用同一个已解析 `data_root` runtime scope。

`power_user.chat_truncation` 只限制首次挂载的 DOM 数量，不裁剪 `chat[]`。`Show more messages` 从完整数组中补挂更早楼层，不发起历史 I/O，也不改变数组索引。后续 DOM virtualization 若实施，也只能替换渲染层，不能改变 canonical data contract。

## 3. 完整保存

所有当前聊天写入都保存完整 header + `chat[]`：

- 角色：`POST /api/chats/save`
- 群聊：`POST /api/chats/group/save`

前端入口必须经过 `enqueueChatSave()`，保证进程内聊天保存有序。facade 使用 target-local commit session 分块传输 JSONL，finish 阶段校验 ACK 并原子发布；角色和群聊继续保留各自的 integrity、metadata 与事件语义。

保存失败必须向调用者传播。不存在局部 patch 失败后静默改走另一条写路径的降级逻辑。

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

Chat Completion preset 的“剥离旧函数调用结果”只改变 provider prompt：开启后，最后一条用户消息之前的完整工具轮（包括发起调用的 Assistant 消息及全部 Tool results）都会省略，只保留不含工具调用的普通 Assistant 消息。当前用户消息之后的递归工具链仍完整重放。该选项不修改 `chat[]`、DOM 或持久化历史，也不作用于 Agent prompt assembly。

工具执行中的即时反馈只属于前端运行态：整批 calls 校验通过后，UI 在 owner Assistant 上显示 pending cards，并随每个工具完成更新结果；所有工具结束后，pending 消失，持久化 Tool 楼层按自身物理位置显示。pending 状态不进入 `chat[]`、不保存、也不提前发出工具事件，但会在 ChatSurface 重挂载时按同一 Assistant 对象恢复。

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
- tail/before 对角色和群聊返回相同索引语义，stale cursor 明确失败。
- 旧 settings 中的 `chat_history_mode` 被 serde 作为未知字段忽略，重新序列化时不会保留。
