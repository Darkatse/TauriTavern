# TauriTavern Agent Tool System

本文档定义 Agent Tool System 的 Registry、Policy、Tool Call、Tool Result、审批与前端/扩展/MCP 边界。

Agent 的能力上限很大程度由工具决定，但第一期更重要的是工具架构正确，而不是工具数量多。

## 1. 目标

工具系统必须做到：

- 工具可发现。
- 工具可按 profile/plan/policy 控制可见性。
- 工具调用可审计。
- 工具结果能进入 ContextFrame。
- 工具错误能被模型和用户理解。
- 工具副作用可 checkpoint/rollback 或明确不可回滚。
- MCP/extension/内置工具使用同一抽象。

## 2. 非目标

第一期不做：

- shell 工具。
- 任意后端 JS 执行。
- 世界书动态脚本作为后端工具。
- 任意远端工具自动注册。
- MCP Sampling 自动模型调用。
- 大而全的插件市场。

## 3. ToolSpec

建议模型：

```rust
ToolSpec {
    name,
    title,
    description,
    input_schema,
    output_schema,
    annotations,
    visibility,
    permission,
    budget,
    source,
}
```

字段说明：

- `name`：稳定 ID，例如 `workspace.apply_patch`。
- `title`：UI 展示名。
- `description`：给模型和用户看的能力说明。
- `input_schema`：JSON Schema。
- `output_schema`：可选 JSON Schema。
- `annotations`：side effect、read only、destructive、idempotent、cost 等。
- `visibility`：模型是否可见、是否只对用户可见。
- `permission`：always allow、approval required、deny。
- `budget`：最大调用次数、最大输出 token、超时。
- `source`：built_in、mcp、extension、skill。

## 4. ToolResult

建议模型：

```rust
ToolResult {
    call_id,
    content,
    structured,
    is_error,
    resource_refs,
    usage,
}
```

`content` 支持：

```text
Text
Json
ImageRef
AudioRef
FileRef
ResourceRef
DiffRef
```

原则：

- 大结果写 resource ref，不内联到 journal。
- `is_error = true` 可以是模型可恢复错误，不一定让 run Failed。
- 系统级错误，如 workspace path escape、journal append failed，必须让 run Failed。

## 5. Tool Call 生命周期

```text
model emits tool call
  ↓
parse to ToolCall
  ↓
policy resolve
  ↓
maybe approval
  ↓
dispatch
  ↓
write result
  ↓
append journal
  ↓
ContextFrame includes ToolResults if policy allows
```

Journal：

```text
tool_call_requested
tool_call_awaiting_approval
tool_call_approved / tool_call_denied
tool_call_started
tool_call_completed / tool_call_failed
```

## 6. Policy Resolution

输入：

- user global policy
- profile tool policy
- plan node tool policy
- tool source policy
- platform policy
- runtime budget

输出：

```text
visible: bool
callable: bool
approvalRequired: bool
reason
budget
```

规则：

- user deny 最高。
- platform deny 不可覆盖。
- plan node deny/allow 优先于 profile allow。
- deny 优先 allow。
- approval 不是 deny。
- 未允许工具默认不可见。

Policy violation 必须写 journal 并 fail-fast，除非这是模型可恢复的 denied tool result 策略。

## 7. 内置工具

### 7.0 当前实现

截至 2026-05-30，当前 registry 开放 agent / chat / world info / dice / skill / workspace 六类内建工具。Registry 只产 canonical `AgentToolSpec`，provider-facing schema/alias 由 gateway 边界转换。Profile 会在运行时过滤 visible tool list，并可替换 model-facing ToolSpec copy 的工具 description 与参数 description；canonical specs 不被修改。return-mode child invocation 会在 runtime 层收窄工具面：移除 chat commit / run finish / delegation tools，并注入 runtime-only `task.return`。child 仍使用私有 `summaries/` / `scratch/` 语义目录；共享 `output/`、`plan/`、`persist/` 等 root 的可见/可写能力由 target Profile workspace policy 决定。

Agent-facing 文案必须从调用或执行 Agent 的角度描述可操作路径：`agent.delegate` 鼓励在 task brief 中给出相关 workspace path 与期望 artifact；return-mode workspace tools 只提示 visible/writable roots 与 task-facing path，不暴露 physical mapping、CAS 参数或 runtime id。

| Canonical name | Model-facing alias | Side effect | 状态 |
| --- | --- | --- | --- |
| `agent.list` | `agent_list` | 只读列出当前 Agent 可调用的 Agent Profile 目录 | 已落地 |
| `agent.delegate` | `agent_delegate` | 创建 return-mode 子任务，不立即返回结果 | 已落地 |
| `agent.await` | `agent_await` | 查询或等待当前 invocation 创建的 delegated task 结果 | 已落地 |
| `task.return` | `task_return` | return-mode child invocation 提交任务结果并结束 child work；runtime-only，不允许写入 profile tools.allow | 已落地 |
| `chat.search` | `chat_search` | 只读搜索当前聊天，返回 message index、snippet 与 ref | 已落地 |
| `chat.read_messages` | `chat_read_messages` | 只读按 message index 读取当前聊天消息，可读取字符范围 | 已落地 |
| `worldinfo.read_activated` | `worldinfo_read_activated` | 只读读取本轮 run 捕获的最终激活世界书条目 | 已落地 |
| `dice.roll` | `dice_roll` | 只读随机投骰；支持 `d6`、`1d20`、`3d6+4` 等轻量 dice notation，默认 Profile 不启用 | 已落地 |
| `skill.list` | `skill_list` | 只读列出已安装 Skill 索引摘要 | 已落地 |
| `skill.search` | `skill_search` | 只读搜索单个可见 Skill 内的 UTF-8 文本文件，返回 snippet 与 ref | 已落地 |
| `skill.read` | `skill_read` | 只读读取已安装 Skill 内的 UTF-8 文本文件或范围 | 已落地 |
| `workspace.list_files` | `workspace_list_files` | 只读列出模型可见 workspace 文件 | 已落地 |
| `workspace.search_files` | `workspace_search_files` | 只读搜索模型可见 workspace UTF-8 文本文件，返回 snippet 与 ref | 已落地 |
| `workspace.read_file` | `workspace_read_file` | 只读读取 UTF-8 文本或范围，完整读取记录 read-state | 已落地 |
| `workspace.write_file` | `workspace_write_file` | 写 run workspace 文件，成功后 checkpoint | 已落地 |
| `workspace.apply_patch` | `workspace_apply_patch` | 单文件精确替换，成功后 checkpoint | 已落地 |
| `workspace.commit` | `workspace_commit` | 将 workspace 文件提交到当前 chat message | 已落地 |
| `workspace.finish` | `workspace_finish` | 结束工具循环，进入 run 收尾 | 已落地 |

默认模型可见/可写 workspace 前缀为：

```text
output/
scratch/
plan/
summaries/
persist/
```

这些前缀由 resolved Profile 写入 run manifest，Profile 只能收窄 root universe。`persist/` 是 chat workspace 级持久 root 的 run projection：模型在 run 中通过普通 workspace 工具写入，`workspace.finish` 收尾成功后才 promote 回稳定 chat workspace；失败或取消的 run 不会写回。

工具参数会写入 `tool-args/call_<sha256_8byte_hex(call-id)>.json`，工具结果会写入 `tool-results/call_<sha256_8byte_hex(call-id)>.json`；本地文件名只使用 SHA-256 前 8 字节 hex。provider 返回的原始 `call_id` 只作为不透明业务 ID 保存在 JSON 内容、journal payload 与下一轮 canonical `ToolResult` part 中，不作为本地文件名。Gateway 会在 provider 边界把它转换为对应 provider 格式。工具结果不会写入 SillyTavern chat 楼层。

`workspace.apply_patch` 使用 Claude Code 风格的 `old_string` / `new_string` 单文件精确替换。未完整读取、版本变化、匹配 0 次或多次会作为 recoverable tool error 返回模型。`workspace.write_file` 对已存在文件复用同一个 session read-state 做 CAS：模型不需要传 `expectedSha256`，schema 不暴露 overwrite policy；若文件在最近读取/写入后被其他 invocation 修改，会返回可恢复的 stale-file 工具错误，要求重新读取后再写。模型传入的非法 path、空 path、不可见/不可写 path 也作为可恢复工具错误回填；目标 path 实际指向目录的读写请求会作为 `workspace.path_is_directory` 业务错误回填，提示模型改用 `workspace_list_files`。repository 内部 escape/symlink/journal、checkpoint、序列化、取消和模型响应结构错误仍 fail-fast。

`workspace.commit` 与 `workspace.finish` 的契约：模型可以多次 commit；当全部修订与 commit 完成后，必须用 `workspace.finish` 收口，不能用纯文本代替最终 answer。`workspace.commit` 工具的返回字符串只做温和提醒，提示模型可继续修订并再次 commit，但最终不要忘记 finish。

如果模型一回合内仍然返回 0 个 tool_calls（drift），loop runner 会先做一次"软纠正"（issue #64）：把模型的纯文本回复捕获到 workspace 的 `direct_output.md`（默认 `output/direct_output.md`，实际跟随当前 profile 的 messageBody artifact root），写 `direct_output_captured` 与 checkpoint；再把该 assistant 回复推进 history，并追加一条 `user` 角色的合成提醒，让模型在下一轮通过 `workspace_commit` / `workspace_finish` 补回流程。**每个 run 至多 1 次**（常量 `DRIFT_RECOVERY_MAX_ATTEMPTS`），每次都会写一条 `drift_recovery_attempted` 事件。

- 软纠正后模型调用了 `workspace_finish`，或继续修订并再次 `workspace_commit` 后再 `workspace_finish` → run 继续，无 rollback。
- 软纠正失败（模型再次 0 tool_calls）→ 回落到 `model.tool_call_required` 失败路径；若没有成功 chat commit，则写 `run_failed`（`userRetryable=true`）；若已有成功 chat commit，则写 `run_partial_success`，保留已提交聊天输出并以 warning 暴露底层错误。

违反此契约的其它形态（`agent.tool_after_finish` / `agent.max_tool_rounds_exceeded`）目前不走软纠正，直接进入同一终态分类：没有成功 chat commit 时 fail-fast；已有成功 chat commit 时 `run_partial_success`。细节见 `RunEventJournal.md`。

当前没有 MCP、shell、extension bridge、profile routing、Plan Mode runtime、模型可见 task cancel、`agent.handoff` 或审批工具。

### 7.1 Agent Delegation Tools

`agent.list`

- Read-only。
- 根据当前 Profile delegation policy 与 target Profile callable policy，列出可调用 Agent。
- 支持 `purpose = any | delegate | handoff`、`query`、`limit`。
- 当前 handoff 仍是后续计划；在 `agent.handoff` 落地前，生产 Profile 不应把 handoff 能力作为可执行路径交给模型。

`agent.delegate`

- Mutating/control。
- 创建 return-mode `AgentTaskRecord` 与 child `AgentInvocation`。
- 参数只包含 `agentId`、`task` 与可选 `budget`；不接受 `execution`、`continuation`、`invocationId` 等 runtime 字段。
- 子任务提交给当前 run 的 `AgentTaskScheduler` 后台执行；父 Agent 不需要阻塞在 `agent.delegate` 上。

`agent.await`

- Read-only/control。
- 查询或等待当前 invocation 自己创建的 delegated tasks。
- 不驱动 queued task 执行；后台 worker 完成后，`agent.await` 只负责等待/渲染已有状态。
- 返回 markdown result capsule，并保留 structured result 给 journal/audit。
- 即使父 Agent 不显式 await，terminal child results 也会在下一次父 Agent tool turn 后注入下一轮模型请求。
- `taskIds` 是可选的精确句柄；省略时面向当前调用者自己启动的 delegated tasks。

`task.return`

- return-mode child invocation 专用。
- Profile 不得显式允许该工具；runtime 根据 `TaskReturnRequired` exit policy 注入。
- 调用后写入 `agent-results/<child-invocation-id>.json` 与 `summaries/agents/<workspace-key>/result.md`。
- child invocation 必须用它结束工作，不能用 `workspace.finish`。

### 7.2 后续内置工具候选

后续优先补齐策略与 checkpoint 工具：

```text
workspace.create_checkpoint
```

### 7.3 Workspace Tools

`workspace.list_files`

- Read-only。
- 返回 workspace tree。
- 可按 path prefix。

`workspace.search_files`

- Read-only。
- 只搜索 manifest 中模型可见的 workspace roots，例如 `persist/`、`summaries/`、`plan/`、`scratch/`、`output/`。
- 输入为 `query`，可选 `path`、`limit`、`context_lines`。
- 返回 path、score、line range、snippet、sha256 与 `workspace:<path>#Lx-Ly` ref。
- 0 命中是成功结果；非法 path、不可见 path、缺失 path 是 recoverable tool error。
- 不搜索 `input/`、`tool-results/`、`model-responses/`、`checkpoints/`、`events.jsonl` 等隐藏 runtime 存储。

`workspace.read_file`

- Read-only。
- 只能读 visible resource。
- 支持 `start_line` / `line_count` 行范围，也支持 `start_char` / `max_chars` 字符范围；两种范围不能混用。
- 完整读取会记录 read-state；部分读取只用于定位和理解，不解锁 `workspace.apply_patch`。
- 受内部 byte 上限、line 与 partial char 上限控制。

`workspace.write_file`

- Mutating。
- 只能写 writable path；return-mode child 应把 `summaries/` / `scratch/` 用作私有笔记，只在任务要求 artifact 或 edit 时写共享 writable roots。
- 写后应 checkpoint。
- 已存在文件若发生并发修改，会返回 recoverable stale-file tool error；模型重新读取后再写。

`workspace.apply_patch`

- Mutating。
- 应使用明确 patch 格式，且编辑已有文件前必须完整读取。
- patch 失败返回可恢复 tool error。
- path escape 是 system failure。

`workspace.create_checkpoint`

- Mutating metadata。
- 可由 runtime 自动调用，也可暴露给模型。

`workspace.commit`

- 控制/Mutating 工具。
- 无参数默认将 `output/main.md` 以 `replace` 提交到当前 chat message。
- `append` 在本 run 无既有 commit 时创建消息，之后始终追加同一消息楼层。
- 实际 chat 写入必须通过前端 host bridge 调用上游 `saveReply()`。

`workspace.finish`

- 控制工具。
- 表示模型认为本次 run 已完成。
- 前台 run 在 finish 前必须至少成功 `workspace.commit` 一次；后台 run 可以无 commit。
- Runtime 在 finish 收尾阶段提交 `persist/` projection。
- return-mode child invocation 不可用；child 必须使用 `task.return`。
- 当前允许在 unfinished child task 存在时结束 root run；finish 会默认取消当前 parent 拥有的 unfinished child tasks，run 收尾会取消剩余 unfinished child tasks。

### 7.4 Chat Tools

`chat.search`

- Read-only。
- 通过 Rust chat repository / group chat repository search 能力实现。
- 只能搜索当前 run 绑定的聊天，不允许模型传入任意 chat target。
- 只有 `query` 必填；`limit`、`role`、`start_message`、`end_message`、`scan_limit` 都是可选参数。
- 返回 message index、role、score、snippet 与 `chat:current#<index>` ref。
- 0 命中是成功结果，不是 recoverable error。
- 不能把完整 history 拉入前端。

`chat.read_messages`

- Read-only。
- 通过 0-based message index 精确读取当前聊天消息。
- 输入使用 `messages: [{ index, start_char?, max_chars? }]`，降低 LLM 心智负担，避免 page/cursor 等不必要抽象。
- 一个工具调用最多读取 20 条消息。
- 单条完整读取上限 8000 字符；长消息必须用 `start_char` / `max_chars` 分段读取。
- 总返回上限 20000 字符。
- message index 不存在、范围非法、读取过大属于 recoverable tool error。
- chat JSONL header 不计入 message index；第一条聊天消息 index 为 0。

### 7.5 Skill Tools

`skill.list`

- Read-only。
- 返回当前 Profile 可见的已安装 Skill 索引摘要。
- `skills.deny` 优先于 `skills.visible`；`visible: ["*"]` 表示全部已安装 Skill 可见。
- 不读取 `SKILL.md` 全文。

`skill.search`

- Read-only。
- 输入为 `name`、`query`，可选 `path`、`limit`、`context_lines`。
- 只能搜索当前 Profile 可见且未 deny 的单个 Skill。
- 搜索结果只返回 snippet 与 ref，不返回完整文件。
- 返回 snippet 字符数计入 Profile 的 Skill run read budget，防止绕过 `skill.read` 预算。
- 二进制文件会显式计入 skipped files；非法 path、缺失 path、不可见 Skill 或预算耗尽是 recoverable tool error。

`skill.read`

- Read-only。
- 输入为 `name`、可选 `path`、`start_line`、`line_count`、`start_char`、`max_chars`。
- `path` 默认 `SKILL.md`，必须是 Skill 内相对路径。
- 支持行范围与字符范围；两种范围不能混用。
- 只能读取 Profile 可见的 UTF-8 文本；二进制、缺失文件、非法路径、symlink escape、不可见 Skill 或超预算读取都是 recoverable tool error，除非 repository 内部 IO / index 损坏等宿主级问题需要 fail-fast。
- `max_chars` 受 Profile 的 `maxReadCharsPerCall` 与 `maxReadCharsPerRun` 控制。
- 结果进入 journal / tool result / 下一轮 model request。Skill 原始文件保持 read-only；模型需要摘录或改写时应写入 `scratch/`、`summaries/` 或 `output/`。

### 7.6 WorldInfo Tools

`worldinfo.read_activated`

- Read-only。
- 读取本次 run materialized 的 `promptSnapshot.worldInfoActivation`。
- `startRunFromLegacyGenerate()` 从本轮 dryRun 的最终 `WORLDINFO_SCAN_DONE` 捕获该快照。
- 无参数调用只返回本轮激活条目的索引：`ref`、条目名、世界书名、位置与正文字符数，不返回正文。
- 读取正文必须传入 `entries: [{ ref, start_char?, max_chars? }]`，其中 `ref` 来自无参数索引结果；长正文使用 `start_char` / `max_chars` 分段读取。
- 模型可读 `content` 保持简洁：索引模式给条目列表，正文模式只给被明确请求的条目内容。
- 结构化结果可以保留 `uid`、`position`、`ref`、`timestampMs` 等 audit 字段，但不要把这些作为模型阅读主内容。
- 不暴露 world info 扫描中间循环状态为 Public Contract。

## 8. Provider Tool Call Adapter

不同 provider tool call 格式不同。Tool System 内部必须使用统一格式：

```text
ToolCall {
  id,
  name,
  arguments,
  providerMetadata,
}
```

Provider adapter 负责：

- 把 `ToolSpec` 转成 provider schema。
- 把 provider-native tool call 转回 `ToolCall`。
- 保留必要 native metadata，例如 reasoning signature、tool call id。
- 缺失 tool call id 时 fail-fast，不生成 fallback id。

上层 runtime 不应关心 OpenAI/Claude/Gemini 的工具字段差异。

## 9. Tool Result 进入 Prompt

工具结果不写 chat message。

工具结果进入后续模型请求的路径：

```text
ToolResult store
  ↓
ContextAssemblyService
  ↓
PromptComponentKind::ToolResults
  ↓
ModelRequest
```

Preset 可以控制：

- tool result 是否可见。
- 原文还是摘要。
- 预算。
- 与 chat history/world info/workspace file 的顺序。

## 10. Approval

需要审批的工具：

- MCP tools。
- destructive tools。
- commit/rollback。
- external network side effects。
- 高成本模型/采样工具。

Approval UI 至少展示：

- tool name/title。
- arguments summary。
- side effect annotation。
- source。
- policy reason。

审批结果必须写 journal。

## 11. Error Semantics

工具错误分三类：

```text
RecoverableToolError
  模型可读结果，run 可继续。

PolicyDenied
  根据 policy 决定 fail-fast 或返回 denied tool result。

SystemFailure
  runtime 失败，run 进入 Failed。
```

例子：

- `chat.search` 查不到结果：successful empty result。
- `workspace.apply_patch` patch context mismatch：recoverable error。
- `workspace.read_file` path traversal 参数：recoverable invalid path tool error。
- denied MCP tool：policy denied；是否 recoverable 由该工具的 policy 决定。
- journal append failed：system failure。

## 12. 与 Legacy ToolManager 的关系

当前前端 `ToolManager` 是 Legacy Generate 的工具系统。它：

- 在前端注册工具。
- 直接调用 JS action。
- 把结果保存成 `is_system` chat message。
- 递归调用 `Generate()`。

Agent Tool System 不能复用它作为运行时真相。

可以借鉴：

- function tool 的作者体验。
- display name / format message。
- provider tool schema 注册经验。

禁止继承：

- 工具结果写 chat 楼层。
- 递归 Generate 驱动循环。
- 后端执行任意 JS。

## 13. Extension Tool Bridge

未来扩展工具应通过受控 bridge：

```text
extension registers tool metadata
  ↓
ToolRegistry marks source=extension
  ↓
Agent requests tool
  ↓
frontend bridge asks extension to execute
  ↓
result returns to backend journal
```

要求：

- extension tool 默认需要用户或 profile 授权。
- bridge 调用必须有 timeout。
- result 必须结构化。
- extension 不得直接写 Agent workspace，必须通过 tool result 或受控 workspace tool。

## 14. 当前 Tool System 基线

当前已经具备真正多轮 Agent loop 所需的最小工具系统：

- `ToolSpec` / `ToolCall` / `ToolResult` domain model。
- Rust-owned builtin registry。
- canonical tool specs 与 provider-safe model alias。
- Agent runtime 使用 canonical `ToolCall` / `ToolResult` part，provider 边界负责格式转换。
- chat search/read_messages、worldinfo read_activated、dice roll、skill list/read、workspace list/read/write/apply_patch/finish。
- agent list/delegate/await 与 runtime-only task.return 的 return-mode SubAgent MVP。
- tool arguments / tool results resource refs。
- recoverable tool error 回填模型。
- 前 5 轮 workspace write/patch 成功结果会 hydrate 完整文件内容到下一轮模型上下文。
- workspace mutation checkpoint。
- journal events。

下一步新增 MCP 或 extension bridge 工具时，应复用这一套 registry/dispatcher/result/error 语义，而不是新建旁路。Skill policy 已留在现有 `skill.list` / `skill.search` / `skill.read` dispatcher 与 profile resolver 之间；后续不要新建第二套 Skill 读取入口。
