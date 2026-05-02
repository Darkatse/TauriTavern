# Agent Framework 当前状态

本文档是 Agent 框架的当前事实入口。后续开发先读本文，再读 `docs/AgentArchitecture.md`、`docs/AgentContract.md`、`docs/AgentImplementPlan.md` 与 `docs/Agent/` 下的专题文档。

旧阶段性施工说明已经吸收为当前基线，不再作为开发入口；需要历史背景时只看 git history。

## 当前基线

截至 2026-05-02，Agent 当前基线：

- Rust 后端已有 Agent domain model、runtime、workspace、journal、checkpoint、commit bridge。
- 前端已挂载 `window.__TAURITAVERN__.api.agent` 最小 Host ABI。
- Agent 启动仍通过 `PromptSnapshot` 兼容桥进入；`GenerationIntent + ContextFrame` 尚未接管上下文组装。
- LLM 调用仍复用 `ChatCompletionService::generate_exchange_with_cancel()`，不得绕过现有 provider、secret、日志、endpoint policy、iOS policy、prompt cache 或取消链路。Responses WebSocket 建连已收敛到 `HttpClientPool` 的 ChatCompletion WebSocket profile，见 `docs/CurrentState/NativeApiFormats.md`。
- Agent runtime 已不再把 OpenAI-compatible raw JSON 当作内部事实；运行时使用 canonical `AgentModelRequest` / `AgentModelResponse` / `AgentModelMessage` / `AgentModelContentPart`。
- `AgentModelGateway` 在 Agent canonical IR 与现有 `ChatCompletionGenerateRequestDto` 之间转换；provider-native metadata 作为 opaque `Native` part 保留。
- `provider_state` 已是 run-scoped continuation contract；OpenAI Responses 使用它驱动 persistent WebSocket、incremental input 与 `previous_response_id`。
- 当前工具循环是非 streaming；provider stream 仍不是 Agent timeline event。
- Legacy Generate 尚未默认切到 Agent；Agent Mode off 时上游 SillyTavern 生成、事件和保存语义不变。

## 当前 Host ABI

已落地：

```ts
api.agent.startRunFromLegacyGenerate(input?)
api.agent.startRunWithPromptSnapshot(input)
api.agent.subscribe(runId, handler, options?)
api.agent.cancel(runId)
api.agent.readEvents(input)
api.agent.readWorkspaceFile(input)
api.agent.prepareCommit(input)
api.agent.commit(input)
api.agent.finalizeCommit(input)
```

明确不存在公共 `api.agent.startRun()` alias。启动入口必须表达 prompt 来源：

- `startRunFromLegacyGenerate()`：调用 Legacy `Generate(..., dryRun = true)`，捕获 `GENERATE_AFTER_DATA` 中的 `generate_data` 与本轮最终 `worldInfoActivation`。
- `startRunWithPromptSnapshot()`：调用方已经持有 `promptSnapshot.chatCompletionPayload`，可选携带 `promptSnapshot.worldInfoActivation`。

当前显式拒绝：

- `stream: true`
- `autoCommit: true`
- prompt snapshot 中已有 external `tools`
- external `tool_choice`
- 已有 `role: "tool"` 或 assistant `tool_calls`

## 当前工具集

Tool registry 只产 canonical `AgentToolSpec`。Provider-facing alias 由 gateway/payload adapter 渲染，不再由 registry 暴露 OpenAI-shaped tools。

| Canonical name | Model alias | 类型 | 当前语义 |
| --- | --- | --- | --- |
| `chat.search` | `chat_search` | read-only | 搜索当前 run 绑定的聊天。只有 `query` 必填；可选 `limit`、`role`、`start_message`、`end_message`、`scan_limit`。 |
| `chat.read_messages` | `chat_read_messages` | read-only | 按 0-based message index 读取当前聊天消息；每项可选 `start_char`、`max_chars`。JSONL header 不计入 index。 |
| `worldinfo.read_activated` | `worldinfo_read_activated` | read-only | 读取本次 Agent run 捕获的最终激活世界书条目，不读取全局 last activation。 |
| `workspace.list_files` | `workspace_list_files` | read-only | 列出模型可见 workspace 文件。`path` 省略、空字符串、`.`、`./` 表示 workspace root。 |
| `workspace.read_file` | `workspace_read_file` | read-only | 读取 UTF-8 文本文件并返回行号；完整读取会记录 read-state。 |
| `workspace.write_file` | `workspace_write_file` | mutating | 写完整 UTF-8 文件；成功后记录 read-state 并创建 checkpoint。 |
| `workspace.apply_patch` | `workspace_apply_patch` | mutating | 单文件 `old_string` / `new_string` 精确替换；要求已完整读取或由本 run 创建/修改。 |
| `workspace.finish` | `workspace_finish` | control | 结束工具循环；默认 final artifact 是 `output/main.md`。 |

当前没有 `skill.list`、`skill.read`、MCP 工具、shell 工具、外部 extension tools、tool approval 或 profile routing。

## Model Gateway 当前事实

当前 Agent model 边界：

```text
AgentRuntimeService
  -> AgentModelGateway.generate_with_cancel(AgentModelRequest)
    -> encode_chat_completion_request()
      -> ChatCompletionService.generate_exchange_with_cancel(ChatCompletionGenerateRequestDto)
        -> provider payload builder / repository / logging wrapper
    -> decode_chat_completion_response()
  -> AgentModelResponse
```

Canonical IR 位于 domain model：

```rust
AgentModelRequest {
    payload,
    messages,
    tools,
    tool_choice,
    provider_state,
}

AgentModelContentPart {
    Text,
    Reasoning,
    ToolCall,
    ToolResult,
    Media,
    ResourceRef,
    Native,
}
```

当前实现重点：

- Agent runtime 只消费 `AgentModelResponse.tool_calls`，不再读 `/choices/0/message/tool_calls`。
- Tool call id 必须是 provider 返回的不透明字符串；缺失 `tool_call_id` 会 fail-fast。
- OpenAI Responses 请求会注入 `include: ["reasoning.encrypted_content"]`，以便保留 reasoning continuation 所需 opaque 内容。
- Tool schema 在 gateway 边界按 provider format 做深拷贝 sanitizer；canonical schema 本身不被污染。
- Claude / Gemini / OpenAI Responses / Gemini Interactions 的 native blocks 会进入 normalized `message.native`，再进入 Agent `Native` part。

当前 `provider_state` 契约：

- 初始值是 `{ "sessionId": runId }`。
- 每轮成功后由 `AgentModelGateway` 返回新的 `provider_state`，包含 `sessionId`、`chatCompletionSource`、`providerFormat`、`messageCursor`、`lastResponseId`。
- OpenAI Responses 额外包含 `transport: "responses_websocket"` 与 `previousResponseId`。
- OpenAI Responses 第二轮起只发送 `messageCursor` 之后的新消息，并过滤 assistant messages；缺失或越界 cursor 会 fail-fast。
- native provider 返回 tool call 但缺失对应 native part 时，以 `model.native_metadata_lost` fail-fast。
- ChatCompletion payload 内部使用 `_tauritavern_provider_state` 传递该状态；LLM API log 与真正发往上游的 payload 都会剥离该字段。
- 完整契约见 `docs/CurrentState/AgentProviderState.md`。

当前 native metadata 保留点：

- Claude：保留 assistant `content` blocks，用于回放 `thinking` / `tool_use` / signature。
- Gemini：保留 response `content.parts` 与 `thoughtSignature`。
- OpenAI Responses：保留 raw `output` items 与 `responseId`。
- Gemini Interactions：保留 raw `outputs`，包含 `thought`、`function_call`、URL context 等非通用块。

## Tool Result Context 策略

当前工具结果有两个面：

- journal / workspace 保存的是真实 tool result、tool args、resource refs。
- 下一轮模型上下文使用 canonical `ToolResult` part。

当前已落地 recent hydration：

- 前 5 轮中，`workspace.write_file` 与 `workspace.apply_patch` 成功后，会把目标文件当前完整内容回填到下一轮模型上下文。
- 该回填只影响 model request，不改变实际 workspace/journal 真相。
- hydration 会写 `context_tool_result_hydrated` debug event。

这样避免模型/provider 切换后只看到 `Wrote N bytes...` 而丢失刚写入的真实文本。

## Workspace 与 Commit

当前模型可见 / 可写 roots：

```text
output/
scratch/
plan/
summaries/
persist/
```

`persist/` 是 chat workspace 级持久 root 的 run projection。run 中修改 `persist/` 只影响本 run；`prepareCommit()` 会预检 persistent changes 与并发冲突，`finalizeCommit()` 成功后才 promote 回 `chats/<workspace-id>/persist/`。

Agent commit 当前由前端桥接：

```text
prepareCommit()
  -> 前端 saveReply()
  -> finalizeCommit()
  -> promote persist/
```

Commit 必须遵守 SillyTavern/windowed payload 保存契约，不能直接写 chat JSONL。

## 当前 Run Flow

```text
api.agent.startRunFromLegacyGenerate(input?)
  ↓
Legacy dryRun 捕获 generate_data 与 worldInfoActivation
  ↓
api.agent.startRunWithPromptSnapshot(input)
  ↓
前端解析 chatRef / stableChatId
  ↓
start_agent_run(dto)
  ↓
AgentRuntimeService::start_run()
  ↓
创建 AgentRun / workspaceId / run workspace
  ↓
initialize_run 写 manifest / prompt snapshot / persist projection
  ↓
prepare_agent_tool_request 生成 AgentModelRequest
  ↓
model -> tool -> model -> ... -> workspace.finish
  ↓
workspace mutation 成功后 checkpoint
  ↓
validate_final_artifact(output/main.md)
  ↓
状态进入 awaiting_commit
  ↓
prepareCommit / saveReply / finalizeCommit
```

工具循环最多 80 轮。超过后以 `agent.max_tool_rounds_exceeded` 失败。模型直接输出文本且不调用工具会以 `model.tool_call_required` 失败。

## 当前 Run Events

已落地事件包括：

```text
run_created
generation_intent_recorded
status_changed
workspace_initialized
persistent_projection_initialized
context_assembled
model_request_created
model_completed
tool_call_requested
tool_call_started
tool_result_stored
tool_call_completed / tool_call_failed
workspace_file_written
workspace_patch_applied
checkpoint_created
context_tool_result_hydrated
provider_state_updated
model_response_stored
agent_loop_finished
artifact_assembled
commit_started
persistent_changes_prepared / persistent_changes_prepare_failed
commit_draft_created
persistent_changes_committed / persistent_changes_commit_failed
run_committed
run_completed
run_cancel_requested
run_cancelled
run_failed
```

Provider stream chunk 不是 Agent run event。Agent UI 必须订阅 `api.agent.subscribe(runId, handler)` 的 run event。

## 当前文件布局

```text
_tauritavern/agent-workspaces/
  index/
    runs/
      <run-id>.json
  chats/
    <workspace-id>/
      persist/
        <promoted persistent files>
      runs/
        <run-id>/
          manifest.json
          events.jsonl
          input/
            prompt_snapshot.json
            persist_snapshot.json
          tool-args/
            <tool-call-id>.json
          tool-results/
            <tool-call-id>.json
          model-responses/
            round-XXX.json
          output/
            main.md
          scratch/
          plan/
          summaries/
          persist/
          checkpoints/
            <checkpoint-id>/
              checkpoint.json
              <snapshotted files...>
```

Workspace path 必须是相对路径。绝对路径、Windows drive prefix、NUL、`..` 会被拒绝。工具参数层可修正的问题返回 recoverable tool error；repository 内部 IO、journal、checkpoint、chat JSONL 损坏、序列化、取消和模型响应结构错误是 fatal runtime error。

## 当前手动测试入口

目前没有 UI toggle。前端控制台入口：

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);

const agent = window.__TAURITAVERN__.api.agent;

const run = await agent.startRunFromLegacyGenerate({
  generationType: 'normal',
  options: { stream: false, autoCommit: false },
});

const stop = agent.subscribe(run.runId, event => console.log(event));
```

`startRunWithPromptSnapshot()` 仍可用于低层 smoke，但不要注入 `tools`、`tool_choice` 或已有 tool turns。

## 最近验证命令

最近一次 Rust 侧验证基线：

- `cargo check`
- `cargo test agent_model_gateway`
- `cargo test agent_loop`
- `cargo test openai_responses_payload`
- `cargo test claude_native_content_blocks_are_replayed`
- `cargo test normalize_`
- `cargo test`：470 passed
- `git diff --check`

前端 ABI 本次未修改，未重新运行前端检查。

## 已知待办

- 将 Agent run 接入可控 UI，而不是只靠控制台调用。
- 设计 Agent Mode toggle 与 Legacy Generate 的清晰分流，不改变 Agent Mode off 语义。
- 建立最小 timeline/event viewer。
- 将 `PromptSnapshot` 过渡输入逐步替换为 `GenerationIntent + ContextFrame`。
- 把 `AgentModelGateway` 进一步拆成明确的 provider adapter 模块，减少单文件体积。
- 完成 profile policy：allowed tools、tool budget、tool call mode、provider switch policy。
- 实现 `skill.list` / `skill.read`。
- 实现 readDiff、rollback、listRuns、resume-run、autoCommit/streaming 的明确策略。

## 每次 Agent 相关变更必须更新

新增或修改 Agent 相关实现时，请同步：

- `docs/CurrentState/AgentFramework.md`
- `docs/CurrentState/AgentProviderState.md`
- `docs/AgentImplementPlan.md`
- `docs/Agent/LlmGateway.md`
- `docs/Agent/ToolSystem.md`
- `docs/Agent/RunEventJournal.md`
- `docs/Agent/TestingStrategy.md`
- 涉及 Host ABI 时同步 `docs/API/Agent.md`、`docs/FrontendHostContract.md`、`src/types.d.ts`

## 守护契约

- Agent Mode off 时 Legacy `Generate()` 行为不变。
- LLM 调用不绕过 `ChatCompletionService`、LLM API log、secret、iOS policy、prompt cache；Responses WebSocket 必须继续复用 `HttpClientPool`，不得扩散成新的并行 LLM 调用链。
- Agent runtime 使用 canonical model IR，不把 provider native format 当内部业务事实。
- Provider native metadata 不解析、不清洗、不改写；丢失必要 native metadata 必须 fail-fast 或测试失败。
- Tool call id 是不透明字符串。
- Agent 工具结果不写入 chat 楼层。
- Agent run/timeline event 不伪装成 SillyTavern `GENERATION_*` / `TOOL_CALLS_*` 事件。
- Commit/rollback 遵守 windowed payload 与保存串行化契约。
- MCP stdio command 不由 Agent/Preset/角色卡/世界书直接写入。
