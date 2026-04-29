# TauriTavern Agent Contract

本文档是 Agent 系统的约束文档。后续实现如果和本文冲突，应先修改设计并同步文档，而不是在代码里绕过。

本文使用以下词语：

- 必须：不可破坏的 contract。
- 应该：默认路径；偏离时需要在 PR/文档中解释原因。
- 可以：允许的扩展点。
- 禁止：会破坏兼容性、安全性或可维护性的做法。

## 1. 不变量

### 1.1 Agent 必须写 Workspace，不直接写 Chat

Agent 的中间产物必须写入 workspace：

```text
output/main.md
output/status.md
output/theater.md
plan/plan.md
scratch/*.md
summaries/*.md
```

最终聊天消息只能由 ArtifactAssembly + Committer 统一提交。

禁止：

- LLM response 直接变成 chat message。
- Tool result 直接插入 chat 楼层。
- workspace-mutating tool 绕过 WorkspaceService 写文件。

理由：只有 workspace 才能提供 checkpoint、diff、rollback、artifact assembly 与 timeline 审计。

### 1.2 Run Journal 必须是真相源

所有副作用都必须写入 append-only journal：

- run started / status changed
- context assembled
- model request created / model completed / model error
- tool call requested / approved / completed / failed
- workspace patch applied
- checkpoint created
- profile switched
- summary created
- artifacts assembled
- commit completed / rollback completed
- run cancelled / failed

禁止存在没有 journal 记录的 LLM call、tool call、MCP request、file write、commit。

UI 可以订阅实时 event，但实时 event 只是 journal 的投影。恢复、调试、timeline、回滚必须能以 journal 为核心重建。

### 1.3 Tool Call 是 Run Event，不是 Chat Message

当前 SillyTavern 工具调用会把工具结果保存成 `is_system` chat message，并递归调用 `Generate()`。见 `src/scripts/tool-calling.js:868` 与 `src/script.js:5847`。

Agent 系统必须改为：

```text
ToolCallRequested -> approval? -> ToolCallCompleted -> ContextFrame.ToolResults
```

工具结果需要进入后续 prompt 时，由 ContextAssemblyService 以 `ToolResults` component 纳入，而不是污染 chat history。

### 1.4 Context Flexibility 属于 ContextFrame，不属于 Provider Payload

Preset 的自由度必须表达为 typed prompt components：

```text
ChatHistory
WorldInfo
PresetInstructions
CharacterInstructions
UserProfile
WorkspaceTree
WorkspaceFile
Skill
ToolDefinitions
ToolResults
Plan
DiffSummary
```

Provider adapter 只能把已经编译好的 `ModelRequest` 转换为 OpenAI/Claude/Gemini/DeepSeek/Cohere 等 provider 格式。

禁止在 provider payload builder 中硬编码 TauriTavern Agent 的 prompt 结构。

### 1.5 Agent Profile 是运行策略，不只是模型选择

Agent Profile 必须至少能表达：

- preset ref
- model ref
- prompt/context policy
- visible resource policy
- tool policy
- plan policy
- summary policy
- switch policy
- output/artifact policy
- budget policy

Profile 切换必须记录 journal。Plan node 若锁定 profile，则 runtime 必须拒绝模型自行切换。

### 1.6 Plan 是 Workflow Contract

Plan 不能只是 prompt 里的自由文本。Strict/Hybrid plan 必须转化为 runtime 可检查的节点：

```text
id
locked
profileId
allowedTools
visibleFiles
maxRounds
contextBudget
expectedArtifacts
approvalRequired
```

Plan policy violation 必须显式记录。模型可修正的工具请求可以返回 recoverable tool error；会破坏 plan/state 安全性的违规必须 fail-fast。

### 1.7 Commit 必须走现有 Chat 保存契约

Agent commit 不能直接写 chat JSONL。

必须通过现有 ChatService/ChatRepository 或前端既有保存队列等价路径保存，并遵守 windowed payload 语义：

- UI 只持有 tail window。
- Prompt/history backfill 不扩大 UI chat。
- windowed save/patch 保持 cursor CAS 保护。
- 应用内并发保存必须串行化。

现有前端保存队列见 `src/script.js:536`。windowed payload 保存串行化要求见 `docs/CurrentState/WindowedPayload.md:151`。

### 1.8 Fail Fast，不静默降级

以下情况必须返回明确错误并写 journal；模型工具参数层可修正的问题回填模型，宿主安全/状态机问题 fail-fast：

- workspace path traversal
- missing required artifact
- profile/preset schema invalid
- denied tool requested
- plan locked node 被修改
- hidden resource 被请求进入 context
- cursor integrity conflict
- MCP stdio command 不在 allowlist
- provider/source 被平台 policy 禁用
- provider native metadata、tool call id 或 reasoning signature 被丢弃
- journal append 失败

禁止把这些错误静默转成普通文本、空结果或“继续用默认设置跑”。

### 1.9 LLM 调用必须复用现有服务边界

Agent 第一阶段的 LLM 调用必须经过 `ChatCompletionService` 或它的正式 gateway wrapper。

禁止：

- 直接调用 `HttpChatCompletionRepository`。
- 新建独立 HTTP client 绕过 `HttpClientPool`。
- 绕过 `LoggingChatCompletionRepository` 导致 LLM API log 丢失。
- 绕过 secret/settings/iOS policy/endpoint override/prompt cache。

长期可以抽象 `LlmGatewayService`，但 gateway 的实现必须保留这些现有能力，而不是重写一套较弱路径。

## 2. Clean Architecture Contract

### 2.1 Domain

Domain 可以定义：

- `AgentRun`
- `AgentRunEvent`
- `AgentRunStatus`
- `WorkspaceRef`
- `WorkspacePath`
- `WorkspaceResource`
- `ArtifactSpec`
- `Checkpoint`
- `AgentProfile`
- `PlanPolicy`
- `ToolSpec`
- `ToolResult`
- repository trait
- gateway/dispatcher trait

Domain 禁止依赖：

- Tauri command/channel/window
- concrete filesystem API
- HTTP client implementation
- MCP subprocess implementation
- frontend event names

### 2.2 Application

Application service 可以编排：

- run state machine
- context assembly
- LLM gateway call
- tool dispatch
- workspace patch/checkpoint
- artifact assembly
- commit/rollback

Application service 必须依赖 domain trait 或现有 application service，不直接依赖 infrastructure concrete type。

### 2.3 Infrastructure

Infrastructure 可以实现：

- file workspace repository
- file event journal repository
- checkpoint snapshot/diff store
- skill repository
- MCP client manager
- provider adapter

Infrastructure 必须把平台错误转换为 domain/application error，不允许向上泄露任意底层异常结构。

### 2.4 Presentation

Tauri command 只能做：

- DTO 反序列化与基础校验
- 获取 `AppState`
- 调用 service
- Channel/event 转发
- 错误映射

禁止把 Agent loop 写在 command 函数里。

## 3. Frontend Host ABI Contract

Agent API 必须挂载到：

```js
window.__TAURITAVERN__.api.agent
```

MCP API 必须挂载到：

```js
window.__TAURITAVERN__.api.mcp
```

安装方式必须沿用现有 API 模式：

- `installHostAbi()` 创建 `window.__TAURITAVERN__`。
- 各 API installer 确保 `hostAbi.api` 存在。
- 通过 `context.safeInvoke` 调用 Rust command。
- `subscribe()` 返回幂等 unsubscribe。

禁止：

- 新增公共 `window.__TAURITAVERN_AGENT_*` 散落全局。
- 让第三方直接依赖 Rust command 名。
- 把内部 service object 暴露给 `window.__TAURITAVERN__.api`。

新增类型必须同步 `src/types.d.ts`。

## 4. Legacy Generate Compatibility Contract

Agent Mode off 时必须保持：

- `Generate(type, options, dryRun)` 调用语义。
- `GENERATION_STARTED`、`GENERATION_AFTER_COMMANDS`、`GENERATE_AFTER_DATA` 等事件的可观察顺序。
- slash command、world info、extension prompt、prompt itemization、swipe、regenerate、continue、quiet、impersonate 行为。
- 旧 ToolManager 行为，除非后续单独做兼容迁移并有测试保护。

Agent Mode on 的短期桥接可以使用 dryRun 获取 `PromptSnapshot`。但 Agent tool loop 禁止通过递归 `Generate()` 实现。

dryRun 不能被视为纯函数。它仍会触发上游事件、prompt 组合、World Info dry-run scan、OpenAI prompt ready 等兼容行为。Agent 只能把它当作短期 prompt snapshot 生产器，不能依赖它没有任何副作用。

当前 Agent tool loop 的额外硬约束：

- `Generate(..., dryRun = true)` 不返回 payload；payload 只能从 `GENERATE_AFTER_DATA` 事件捕获。任何调用方把 `await Generate(..., true)` 当作 payload 都是错误用法。
- 公共 Agent 启动入口只有 `api.agent.startRunFromLegacyGenerate()` 与 `api.agent.startRunWithPromptSnapshot()`；不保留职责不清的 `startRun()` alias。
- `startRunFromLegacyGenerate()` 内部可以调用 Legacy dryRun，但工具循环必须在 Rust runtime 中推进，不得递归调用 `Generate()`。
- 工具注册由 Rust runtime 独占；prompt snapshot 中不得携带 external `tools`、`tool_choice`、`role: "tool"` 或已有 `tool_calls`。
- 当前只支持非 streaming、非 autoCommit；请求 `stream: true` 或 `autoCommit: true` 必须 fail-fast。
- 模型可修正的工具参数错误必须作为 `is_error = true` tool result 回填模型；宿主级 IO、journal、checkpoint、序列化、取消和模型响应结构错误必须 fail-fast。

Agent run/timeline/tool event 不得伪装成上游 `GENERATION_*` 或 `TOOL_CALLS_*` 事件。上游事件属于 Legacy Generate 兼容面。

## 5. Windowed Payload Contract

Agent 读取历史必须优先使用后端 chat repository/windowed payload/search 能力。

禁止：

- 把完整 chat history 注入前端常驻 `chat`。
- 为了 Agent 生成而扩大 UI window。
- 在 workspace 中复制完整 JSONL 作为默认行为。
- 绕过 cursor integrity 直接覆盖聊天文件。

允许：

- 将 chat history 暴露为 virtual resource。
- 按 token budget 读取 tail/before/search。
- 在 ContextFrame 中放入历史摘要或检索片段。

`force` 只能用于既有 integrity 覆盖语义，不能跳过 cursor 签名校验。Cursor mismatch 代表 CAS 保护正在阻止不安全写入，不能被静默忽略。

## 6. Workspace Contract

### 6.1 Workspace Identity

Workspace identity 必须基于稳定聊天身份，而不是当前可变引用。

```text
runId = run_<uuid-v4>
stableChatId = api.chat.open(chatRef).stableId()
workspaceId = chat_<sha256(kind + stableChatId)>
```

要求：

- `stableChatId` 必须在前端 Host ABI 层解析并传入 backend DTO。
- backend 可以校验 `stableChatId` 非空并派生 `workspaceId`，但不应直接读取 SillyTavern metadata。
- 每次 normal/regenerate/swipe/continue 都必须创建新的 `runId`。
- 同一 `kind + stableChatId` 的 run 必须共享同一个 chat workspace。
- `chatRef` 只表示当前可定位引用与 commit guard，不得作为 workspace 长期身份。

禁止：

- 用角色显示名、chat file name、完整 `chatRef` hash 作为长期 workspace identity。
- 让 swipe/regenerate 覆盖已有 run journal。
- 因聊天重命名导致同一稳定聊天分裂到多个 chat workspace。

### 6.2 Workspace Path

Workspace path 必须满足：

- UTF-8 relative path。
- 使用 `/` 作为逻辑分隔符。
- 禁止空路径、绝对路径、Windows drive prefix、`..`、NUL。
- 禁止 symlink escape。
- workspace root 外文件不可见。

Workspace resource 分三类：

```text
MaterializedFile
VirtualResource
GeneratedArtifact
```

“万物皆文件”是 Agent 视角的抽象，不代表每个资源都物理复制到 run 目录。

## 7. Tool Contract

工具必须有 `ToolSpec`：

- stable name
- title/display name
- description
- input schema
- optional output schema
- visibility
- permission
- budget
- source

工具结果必须有 `ToolResult`：

- call id
- content blocks
- structured value
- is_error
- resource refs
- usage/cost/duration

工具错误必须能被模型看到，也必须能被用户 timeline 看到。系统错误与模型可恢复错误应区分。

## 8. MCP Contract

MCP 是独立模块，不依附 Agent Mode。

Agent 可以消费 MCP：

```text
MCP Tools     -> ToolRegistry
MCP Resources -> WorkspaceResource / ContextFrame
MCP Prompts   -> PromptComponent
```

禁止：

- Agent/Preset/角色卡/世界书直接创建或修改 MCP stdio command。
- 从远端配置自动创建 stdio server。
- 初期启用 MCP Sampling 的自动模型调用。
- 未经审批调用危险 MCP tool。

## 9. Error Contract

错误返回必须足够结构化，至少包含：

```text
code
message
runId?
eventSeq?
retryable?
details?
```

错误消息可以适合用户阅读，但 code 必须适合测试与扩展判断。

run 失败时必须：

1. journal 追加 `run_failed`。
2. 状态进入 `Failed`。
3. 保留 workspace 与最后 checkpoint。
4. 前端 timeline 可读到失败原因。

cancel 不是 failure，必须进入 `Cancelled`。

## 10. Metadata Contract

Agent commit 到 chat message 时，metadata 必须放入 `extra` 下的 TauriTavern namespace，建议：

```json
{
  "extra": {
    "tauritavern": {
      "agent": {
        "runId": "...",
        "stableChatId": "...",
        "checkpointId": "...",
        "profileId": "...",
        "artifactSetId": "...",
        "artifacts": []
      }
    }
  }
}
```

不要污染上游常用字段名。新增字段必须向后兼容，旧 SillyTavern 或旧扩展忽略后不应崩溃。
