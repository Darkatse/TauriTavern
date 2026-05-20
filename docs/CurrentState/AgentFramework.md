# Agent Framework 当前状态

本文档是 Agent 框架的当前事实入口。后续开发先读本文，再读 `docs/AgentArchitecture.md`、`docs/AgentContract.md`、`docs/AgentImplementPlan.md` 与 `docs/Agent/` 下的专题文档。

历史施工说明已经吸收为当前基线，不再作为开发入口；需要历史背景时只看 git history。

## 当前基线

截至 2026-05-05，Agent 当前基线：

- Rust 后端已有 Agent domain model、runtime、workspace、journal、checkpoint、commit bridge。
- 前端已挂载 `window.__TAURITAVERN__.api.agent` 最小 Host ABI。
- Agent 启动仍通过 `PromptSnapshot` 兼容桥进入；`GenerationIntent + ContextFrame` 尚未接管上下文组装。
- LLM 调用仍复用 `ChatCompletionService::generate_exchange_with_cancel()`，不得绕过现有 provider、secret、日志、endpoint policy、iOS policy、prompt cache 或取消链路。Responses WebSocket 建连已收敛到 `HttpClientPool` 的 ChatCompletion WebSocket profile，见 `docs/CurrentState/NativeApiFormats.md`。
- Agent runtime 已不再把 OpenAI-compatible raw JSON 当作内部事实；运行时使用 canonical `AgentModelRequest` / `AgentModelResponse` / `AgentModelMessage` / `AgentModelContentPart`。
- `AgentModelGateway` 在 Agent canonical IR 与现有 `ChatCompletionGenerateRequestDto` 之间转换；provider-native metadata 作为 opaque `Native` part 保留。
- `provider_state` 已是 run-scoped continuation contract；OpenAI Responses 使用它驱动 persistent WebSocket、incremental input 与 `previous_response_id`。
- Agent Skill 管理、导入导出、embedded skill 提示导入、`skill.list` / `skill.search` / `skill.read` 已落地。
- Phase 3 Agent Profile 基线已落地：`profileId` 会解析为 `ResolvedAgentProfile`，驱动 tools、Skill、workspace roots、output artifact、tool budget、max rounds 与 model-facing prompt/tool descriptions。
- PromptManager 已为 Agent Mode 提供 `agentSystemPrompt` 组装位置与 reserved no-op `agentResults` 位置标记；`agentSystemPrompt` 内容只由 Agent Profile 提供，前端在该 PromptManager index materialize，runtime 只消费最终 messages 并拒绝内部 marker 泄漏；`agentResults` 不再向模型注入历史 commit 内容。
- Profile 仍不接管 provider/model 切换；`preset.ref` 目前只做校验/记录，不改写 prompt snapshot 或 model。
- 当前工具循环是非 streaming；provider stream 仍不是 Agent timeline event。
- Agent System 扩展开关开启时，当前前端会把普通发送、regenerate 与 overswipe 新候选生成接入 Agent；Agent Mode off 时上游 SillyTavern 生成、事件和保存语义不变。
- Agent System 前端已提供 run timeline / detail panel；详情面板顶部可拖动调整高度，高度仅作为扩展 UI 偏好保存，不进入 Agent Host ABI、journal 或 Rust runtime。

## 当前 Host ABI

已落地：

```ts
api.agent.startRunFromLegacyGenerate(input?)
api.agent.startRunWithPromptSnapshot(input)
api.agent.subscribe(runId, handler, options?)
api.agent.cancel(runId)
api.agent.readEvents(input)
api.agent.readWorkspaceFile(input)
api.agent.readModelTurn(input)
```

`startRunFromLegacyGenerate()` / `startRunWithPromptSnapshot()` 支持可选 `profileId`。后端已注册 Profile 管理 Tauri commands（`list_agent_profiles` / `load_agent_profile` / `save_agent_profile` / `delete_agent_profile`），但尚未封装到 `window.__TAURITAVERN__.api.agent` 与 `src/types.d.ts`；正式 UI 属于后续阶段。

Skill 管理 API 已落地：

```ts
api.skill.list()
api.skill.previewImport(input)
api.skill.installImport(request)
api.skill.readFile(input)
api.skill.export(input)
api.skill.exportSkill(input)
```

`api.skill` 是用户/UI/扩展侧的 Skill 管理入口；Agent run 内只通过 `skill.list` / `skill.search` / `skill.read` 工具消费已安装 Skill。

`readModelTurn()` 读取指定 run/round 的模型回合显示 DTO：assistant 输出、可见/摘要化 reasoning、工具调用摘要与 provider 摘要。前端 Timeline 不直接解析 `model-responses/` raw 文件。

明确不存在公共 `api.agent.startRun()` alias。启动入口必须表达 prompt 来源：

- `startRunFromLegacyGenerate()`：调用 Legacy `Generate(..., dryRun = true)`，捕获 `GENERATE_AFTER_DATA` 中的 `generate_data` 与本轮最终 `worldInfoActivation`。
- `startRunWithPromptSnapshot()`：调用方已经持有 `promptSnapshot.chatCompletionPayload`，可选携带 `promptSnapshot.worldInfoActivation`。

当前显式拒绝：

- `stream: true`
- prompt snapshot 中已有 external `tools`
- external `tool_choice`
- 已有 `role: "tool"` 或 assistant `tool_calls`

## Agent Profile 当前事实

Profile 使用 JSON 文件，存储于：

```text
_tauritavern/agent-profiles/
  profiles/<profile-id>.json
  .staging/
```

当前实现边界：

- 缺省 `profileId` 使用 built-in `default-writer`。
- 非缺省 `profileId` 不存在时 fail-fast，不创建 run。
- `instructions.agentSystemPrompt` 省略或为 `null` 时使用 resolved profile 默认 Agent system prompt；设置为非空字符串时完整替换默认 prompt；空白字符串 fail-fast。Preset 控制 `agentSystemPrompt` 的位置与 role，不能编辑其内容。
- `tools.allow` / `tools.deny` 决定模型可见工具，dispatcher 会二次拦截不可见工具。
- `tools.toolDescriptions` 省略或为空时使用默认工具 description；设置时只替换 model-facing ToolSpec copy 的工具总 description 与参数 description。
- `skills.visible` / `skills.deny` 控制 `skill.list`、`skill.search` 与 `skill.read`，`maxReadCharsPerCall` / `maxReadCharsPerRun` 控制 Skill 读取预算。
- `workspace.visibleRoots` / `workspace.writableRoots` 只能收窄 root universe：`output`、`scratch`、`plan`、`summaries`、`persist`。
- `run.presentation` 区分 `foreground` / `background`，默认 built-in profile 为前台；前台 Profile 必须暴露 `workspace.commit`。
- `run.modelRetry` 控制单次模型调用的瞬时错误重试；默认 `maxRetries = 3`、`intervalMs = 3000`。当前只重试 rate limit / transient transport-provider 错误，不重试 prompt/schema/native metadata/tool id 等契约错误。
- `output.artifacts` 当前必须包含且只能包含一个 `messageBody` artifact；`workspace.commit` 默认发布该 artifact 的 path。
- Plan Mode schema 已存在，但当前只支持 `plan.mode = "none"`；其他 mode fail-fast。
- 每个 run 会在 `input/resolved_profile.json` 固化解析结果。

## 当前工具集

Tool registry 只产 canonical `AgentToolSpec`。Provider-facing alias 由 gateway/payload adapter 渲染，不再由 registry 暴露 OpenAI-shaped tools。

| Canonical name | Model alias | 类型 | 当前语义 |
| --- | --- | --- | --- |
| `chat.search` | `chat_search` | read-only | 搜索当前 run 绑定的聊天。只有 `query` 必填；可选 `limit`、`role`、`start_message`、`end_message`、`scan_limit`。 |
| `chat.read_messages` | `chat_read_messages` | read-only | 按 0-based message index 读取当前聊天消息；每项可选 `start_char`、`max_chars`。JSONL header 不计入 index。 |
| `worldinfo.read_activated` | `worldinfo_read_activated` | read-only | 读取本次 Agent run 捕获的最终激活世界书条目，不读取全局 last activation。 |
| `skill.list` | `skill_list` | read-only | 列出当前 Profile 可见的已安装 Skill 索引摘要。 |
| `skill.search` | `skill_search` | read-only | 搜索当前 Profile 可见的单个 Skill 内 UTF-8 文本文件；返回 snippet/ref，snippet 字符数计入 Skill read budget。 |
| `skill.read` | `skill_read` | read-only | 读取当前 Profile 可见 Skill 内的 UTF-8 文本文件或范围；默认 `SKILL.md`，支持 `path`、行范围、字符范围与 `max_chars`，受 Profile read budget 控制。 |
| `workspace.list_files` | `workspace_list_files` | read-only | 列出模型可见 workspace 文件。`path` 省略、空字符串、`.`、`./` 表示 workspace root。 |
| `workspace.search_files` | `workspace_search_files` | read-only | 搜索模型可见 workspace UTF-8 文本文件；可限定 `path`，返回 snippet/ref，不搜索隐藏 runtime 存储。 |
| `workspace.read_file` | `workspace_read_file` | read-only | 读取 UTF-8 文本文件并返回行号；支持行范围和字符范围；完整读取会记录 read-state。 |
| `workspace.write_file` | `workspace_write_file` | mutating | 写完整 UTF-8 文件；成功后记录 read-state 并创建 checkpoint。 |
| `workspace.apply_patch` | `workspace_apply_patch` | mutating | 单文件 `old_string` / `new_string` 精确替换；要求已完整读取或由本 run 创建/修改。 |
| `workspace.commit` | `workspace_commit` | control/mutating | 将可见 workspace 文件提交到当前聊天；无参数等价于 `replace output/main.md`，`append` 首次创建消息、后续追加同一消息。 |
| `workspace.finish` | `workspace_finish` | control | 结束工具循环；前台 run 必须已成功 commit，后台 run 可无 commit。 |

当前没有 MCP 工具、shell 工具、外部 extension tools、tool approval 或 profile routing。

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

- `AgentModelGateway` 已拆为 `agent_model_gateway/` 模块目录：`mod.rs` 保留 trait / `ChatCompletionAgentModelGateway` wrapper；`encode.rs` / `decode.rs` 做 canonical IR 与 normalized ChatCompletion exchange 转换；`format.rs` 解析 source / provider format；`schema.rs` 做 tool schema sanitizer；`provider_state.rs` 管理 continuation；`providers/*` 放 provider-specific adapter 规则。
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

默认模型可见 / 可写 roots：

```text
output/
scratch/
plan/
summaries/
persist/
```

实际 roots 由 resolved Profile 收窄后写入 run manifest。`persist/` 是 chat workspace 级持久 root 的 run projection。run 中修改 `persist/` 只影响本 run；`workspace.finish` 收尾成功时 promote 回 `chats/<workspace-id>/persist/`。

Chat commit 当前由模型显式调用 `workspace.commit` 触发，并由前端 host bridge 执行：

```text
workspace.commit(path?, mode?)
  -> backend 读取 workspace file / checkpoint
  -> chat_commit_requested event
  -> 前端 saveReply(normal | append | appendFinal)
  -> resolve_agent_chat_commit
  -> workspace.finish 成功提交 persist projection 后
  -> persistent_state_metadata_update_requested event
  -> 前端把 persistStateId 写回同一条 chat message
  -> resolve_agent_persistent_state_metadata_update
```

`mode` 默认为 `replace`；`append` 在本 run 尚无 commit 时创建消息，之后多次 commit 始终更新同一个消息楼层。Commit 必须遵守 SillyTavern/windowed payload 保存契约，不能直接写 chat JSONL。`persistStateId` 只能表示已经落盘的 durable persistent state；`chat_commit_requested` 不携带该字段，partial success 保留的聊天输出不会成为下一轮可复用 persist base。

聊天删除现在会联动清理对应的 Agent chat workspace：

- 单个角色聊天删除会按 `chat_metadata.integrity` 派生 workspace id 并删除 `_tauritavern/agent-workspaces/chats/<workspace-id>/`。
- 单个群聊聊天删除会按 group chat id 派生 workspace id 并删除对应 workspace。
- 删除角色且选择删除聊天、删除群组时，会批量清理被删除聊天对应的 Agent workspace。
- 若目标 workspace 仍有当前进程中的 active Agent run，删除会 fail-fast，要求先取消 run；不会先删聊天再留下运行中的 workspace。
- 非 Agent / 旧聊天没有稳定 `integrity` 时不产生 Agent workspace 清理目标，以保持 SillyTavern 删除语义。

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
resolve Profile
  ↓
创建 AgentRun / workspaceId / run workspace
  ↓
initialize_run 写 manifest / prompt snapshot / resolved profile / persist projection
  ↓
prepare_agent_tool_request 按 Profile 生成 AgentModelRequest 与 model-facing tool specs
  ↓
model -> tool -> model -> ... -> workspace.commit? -> workspace.finish
  ↓
workspace mutation 成功后 checkpoint
  ↓
workspace.commit 成功后 host 写入同一条 chat message
  ↓
workspace.finish 结束 run，并提交 persist projection
```

工具循环轮数来自 `profile.tools.maxRounds`。超过后以 `agent.max_tool_rounds_exceeded` 失败。模型直接输出文本且不调用工具会先触发一次 soft drift recovery：runtime 将直接文本捕获到当前 messageBody artifact root 下的 `direct_output.md`（默认 `output/direct_output.md`），记录 `direct_output_captured` 与 checkpoint，然后提醒模型通过 Agent 工具提交/结束；恢复耗尽后仍以 `model.tool_call_required` 失败或 `run_partial_success` 收口。前台 run 在 `workspace.finish` 前必须至少成功 `workspace.commit` 一次；后台 run 可以无 chat commit 结束。

## 当前 Run Events

已落地事件包括：

```text
run_created
profile_resolved
generation_intent_recorded
status_changed
workspace_initialized
persistent_projection_initialized
context_assembled
model_request_created
model_call_attempt_started
model_call_attempt_failed
model_call_retry_scheduled
model_completed
direct_output_captured
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
chat_commit_started
chat_commit_requested
chat_commit_completed / chat_commit_failed
chat_commit_recorded
persistent_changes_committed / persistent_changes_commit_failed
persistent_state_metadata_update_requested
persistent_state_metadata_updated / persistent_state_metadata_update_failed
run_completed
run_partial_success
run_cancel_requested
run_cancelled
run_failed
```

Provider stream chunk 不是 Agent run event。Agent UI 必须订阅 `api.agent.subscribe(runId, handler)` 的 run event。

`model_completed` payload 当前包含 `round`、`modelResponsePath`、`toolCallCount`、assistant/reasoning 字节摘要与 `hasAssistantText` / `hasReasoning`。工具相关事件携带同一 `round`，便于 UI 从任意工具事件跳回本轮模型回合。

`run_partial_success` 是 warning 级终态：当 run 已经有 host-confirmed `workspace.commit`，但之后因 drift、dispatch、persistent commit 或 persistent metadata 写回错误未能干净完成时，保留已提交 chat 输出，并在 payload 中暴露原始错误与 `preservedCommits`。它不是 `run_completed`，也不触发自动 rollback。partial success 消息不会带可复用的 `persistStateId`；下一轮 Agent run 会跳过它，继续寻找更早的 committed persistent state。

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
            resolved_profile.json
            persist_snapshot.json
          tool-args/
            call_<sha256_8byte_hex(tool-call-id)>.json
          tool-results/
            call_<sha256_8byte_hex(tool-call-id)>.json
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
_tauritavern/skills/
  installed/
    <skill-name>/
      SKILL.md
      <skill files...>
  index/
    skills.json
  .staging/
_tauritavern/agent-profiles/
  profiles/
    <profile-id>.json
  .staging/
```

Workspace path 必须是相对路径。绝对路径、Windows drive prefix、NUL、`..` 会被拒绝。工具参数层可修正的问题返回 recoverable tool error；repository 内部 IO、journal、checkpoint、chat JSONL 损坏、序列化、取消和模型响应结构错误是 fatal runtime error。

工具参数与结果的审计文件名使用 provider `tool_call_id` 的 SHA-256 前 8 字节 hex 派生；原始 `tool_call_id` 仍保存在 event payload、审计 JSON 内容与下一轮模型 tool result 中，不能被截断、清洗或替换。

## 当前手动测试入口

Agent System 扩展已在输入栏提供 Agent Mode toggle。开启后，普通发送、`/trigger`、regenerate 菜单与右划 overswipe 生成新候选会走 Agent；普通切换已有 swipe 候选仍保持 Legacy swipe 行为。

`/trigger` 仍保持 SillyTavern 的 `normal` generation 语义，不新增 generation type；但 Agent 路由错误必须 fail-fast，不得回退 Legacy Generate。

前端控制台入口：

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);

const agent = window.__TAURITAVERN__.api.agent;

const run = await agent.startRunFromLegacyGenerate({
  generationType: 'normal',
  // profileId: 'default-writer',
  options: { stream: false, presentation: 'foreground' },
});

const stop = agent.subscribe(run.runId, event => console.log(event));
```

`startRunWithPromptSnapshot()` 仍可用于低层 smoke，但不要注入 `tools`、`tool_choice` 或已有 tool turns。

## 最近验证命令

最近一次 Rust 侧验证基线：

- `cargo fmt --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml agent_runtime_service`：11 passed
- `cargo test --manifest-path src-tauri/Cargo.toml file_agent_repository`：3 passed
- `cargo test --manifest-path src-tauri/Cargo.toml file_agent_profile_repository`：1 passed
- `git diff --check`

最近一次前端侧验证：

- `pnpm run check:frontend`
- `pnpm run check:types`
- `pnpm run check:contracts`：218 passed
- `git diff --check`

## 已知待办

- 将 Agent run 接入可控 UI，而不是只靠控制台调用。
- 建立最小 timeline/event viewer。
- 将 `PromptSnapshot` 过渡输入逐步替换为 `GenerationIntent + ContextFrame`。
- 将后端 Profile 管理 commands 封装到前端 Host ABI 与类型。
- 将 Profile overlay 扩展到 preset / character resolver。
- 明确 provider/model switch policy；当前 Profile 不切换模型。
- 实现 readDiff、rollback、listRuns、resume-run、streaming 的明确策略。

## 每次 Agent 相关变更必须更新

新增或修改 Agent 相关实现时，请同步：

- `docs/CurrentState/AgentFramework.md`
- `docs/CurrentState/AgentProviderState.md`
- `docs/AgentImplementPlan.md`
- `docs/Agent/LlmGateway.md`
- `docs/Agent/ToolSystem.md`
- `docs/Agent/Skill.md`
- `docs/Agent/RunEventJournal.md`
- `docs/Agent/TestingStrategy.md`
- 涉及 Host ABI 时同步 `docs/API/Agent.md`、`docs/API/Skill.md`、`docs/FrontendHostContract.md`、`src/types.d.ts`

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
