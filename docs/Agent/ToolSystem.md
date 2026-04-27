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

### 7.0 Phase 2B 当前实现

截至 2026-04-26，当前 registry 开放五个内建 workspace 工具：

| Canonical name | Model-facing alias | Side effect | 状态 |
| --- | --- | --- | --- |
| `workspace.list_files` | `workspace_list_files` | 只读列出模型可见 workspace 文件 | 已落地 |
| `workspace.read_file` | `workspace_read_file` | 只读读取 UTF-8 文本，完整读取记录 read-state | 已落地 |
| `workspace.write_file` | `workspace_write_file` | 写 run workspace 文件，成功后 checkpoint | 已落地 |
| `workspace.apply_patch` | `workspace_apply_patch` | 单文件精确替换，成功后 checkpoint | 已落地 |
| `workspace.finish` | `workspace_finish` | 结束工具循环，进入 artifact assembly | 已落地 |

模型可见/可写 workspace 前缀当前限制为：

```text
output/
scratch/
plan/
summaries/
```

工具参数会写入 `tool-args/<call-id>.json`，工具结果会写入 `tool-results/<call-id>.json`，并作为 OpenAI-compatible `tool` message 回填下一轮模型请求。工具结果不会写入 SillyTavern chat 楼层。

`workspace.apply_patch` 使用 Claude Code 风格的 `old_string` / `new_string` 单文件精确替换。未完整读取、版本变化、匹配 0 次或多次会作为 recoverable tool error 返回模型。模型传入的非法 path、空 path、不可见/不可写 path 也作为可恢复工具错误回填；repository 内部 escape/symlink/IO、journal、checkpoint、序列化、取消和模型响应结构错误仍 fail-fast。

Phase 2B 当前没有 `chat.search`、`skill.read`、WorldInfo 工具、MCP 或审批工具。

### 7.1 后续内置工具候选

后续建议第一批：

```text
workspace.create_checkpoint

chat.search
chat.read_history_tail
chat.read_history_before

skill.list
skill.read

worldinfo.read_activated
```

### 7.2 Workspace Tools

`workspace.list_files`

- Read-only。
- 返回 workspace tree。
- 可按 path prefix。

`workspace.read_file`

- Read-only。
- 只能读 visible resource。
- 受 token/byte budget 控制。

`workspace.write_file`

- Mutating。
- 只能写 generated/materialized writable path。
- 写后应 checkpoint。

`workspace.apply_patch`

- Mutating。
- 应使用明确 patch 格式。
- patch 失败返回可恢复 tool error。
- path escape 是 system failure。

`workspace.create_checkpoint`

- Mutating metadata。
- 可由 runtime 自动调用，也可暴露给模型。

`workspace.finish`

- 控制工具。
- 表示模型认为 artifact 已完成。
- Runtime 仍必须校验 manifest。

### 7.3 Chat Tools

`chat.search`

- Read-only。
- 通过 Rust chat repository/search 能力实现。
- 不能把完整 history 拉入前端。

`chat.read_history_tail`

- Read-only。
- 读取最近消息窗口。

`chat.read_history_before`

- Read-only。
- 使用 cursor/page 语义读取更早历史。

### 7.4 Skill Tools

`skill.list`

- Read-only。
- 返回当前 profile 可见 skill 的摘要。

`skill.read`

- Read-only。
- 支持 section/budget。
- 结果可进入 ContextFrame。

### 7.5 WorldInfo Tools

`worldinfo.read_activated`

- Read-only。
- 读取最近一次最终激活结果或本次 run materialized 的 world info。
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

- `chat.search` 查不到结果：recoverable result。
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
- provider-safe tool alias 到 canonical name 的映射。
- workspace list/read/write/apply_patch/finish。
- tool arguments / tool results resource refs。
- recoverable tool error 回填模型。
- workspace mutation checkpoint。
- journal events。

下一步新增 `chat.search`、`skill.read`、WorldInfo 或 MCP 工具时，应复用这一套 registry/dispatcher/result/error 语义，而不是新建旁路。
