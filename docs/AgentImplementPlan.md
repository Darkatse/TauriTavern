# TauriTavern Agent Implementation Plan

本文档记录当前可继续开发的实施基线与后续顺序。旧阶段性施工计划已经收敛为当前架构、测试与契约；后续开发不应再从旧阶段文档倒推当前行为。

当前事实以 `docs/CurrentState/AgentFramework.md` 为准，架构边界以 `docs/AgentArchitecture.md` 与 `docs/AgentContract.md` 为准。

## 1. 当前基线

截至 2026-05-02，Agent 当前核心已经落地：

- Rust 后端拥有 Agent domain model、runtime、workspace、journal、checkpoint、commit bridge。
- 前端 Host ABI 已挂载 `window.__TAURITAVERN__.api.agent`。
- Agent 启动仍通过 `PromptSnapshot` 兼容桥进入；`GenerationIntent + ContextFrame` 尚未接管 context assembly。
- `startRunFromLegacyGenerate()` 使用 Legacy dryRun 捕获 `chatCompletionPayload` 与本轮最终 `worldInfoActivation`。
- LLM 调用复用 `ChatCompletionService::generate_exchange_with_cancel()`，不得绕过现有 provider、secret、日志、endpoint policy、iOS policy、prompt cache 和取消链路。Responses WebSocket 与 HTTP client pool 的 proxy / timeout parity 是当前传输层待硬化项。
- Tool loop 由 Rust runtime 独占推进，不递归调用前端 `Generate()`。
- Agent runtime 已使用 canonical model IR，不再把 OpenAI-compatible raw JSON 当作运行时事实。
- `provider_state` 已用于 run-scoped continuation。OpenAI Responses 通过它驱动 persistent WebSocket、incremental input 与 `previous_response_id`。

旧阶段只保留为这些不变量：

- Agent Mode off 时，上游 SillyTavern `Generate()`、ToolManager、事件顺序与 chat 保存语义不变。
- `stableChatId` 是长期聊天身份；`workspaceId` 由 `kind + stableChatId` 派生；`runId` 表示单次执行。
- 所有 run event 进入 append-only journal，不伪装成上游 `GENERATION_*` / `TOOL_CALLS_*` 事件。
- 工具结果进入 workspace / journal / 下一轮 model request，不写入 chat 楼层。
- 最终聊天写入由前端 commit bridge 走 SillyTavern `saveReply()`。
- `PromptSnapshot` 是兼容桥，不是长期上下文架构。

## 2. 当前 Host ABI

已落地入口：

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

明确不存在公共 `api.agent.startRun()` alias。

当前 future API 只保留显式 throw：

```ts
approveToolCall()
listRuns()
readDiff()
rollback()
```

## 3. 当前 Model Gateway

当前 Agent model 调用链：

```text
AgentRuntimeService
  -> AgentModelGateway.generate_with_cancel(AgentModelRequest)
    -> encode_chat_completion_request()
      -> ChatCompletionService.generate_exchange_with_cancel(ChatCompletionGenerateRequestDto)
    -> decode_chat_completion_response()
  -> AgentModelResponse
```

当前 canonical IR：

- `AgentModelRequest`
- `AgentModelResponse`
- `AgentModelMessage`
- `AgentModelContentPart`

`AgentModelContentPart` 当前支持：

- `Text`
- `Reasoning`
- `ToolCall`
- `ToolResult`
- `Media`
- `ResourceRef`
- `Native`

当前已落地：

- provider format detection：OpenAI-compatible、OpenAI Responses、Claude Messages、Gemini、Gemini Interactions。
- canonical tool specs 到 provider-facing function tools 的转换。
- provider-specific schema sanitizer。Gemini / Gemini Interactions 会移除当前不兼容的 JSON Schema 关键字；Claude 只做轻量清洗；OpenAI / Responses 保持完整 schema。
- OpenAI Responses 请求自动 include `reasoning.encrypted_content`。
- Agent OpenAI Responses 续接会使用 `provider_state.previousResponseId` 注入 `previous_response_id`，并用 `messageCursor` 只发送新消息。
- Agent payload 内部字段 `_tauritavern_provider_state` 不进入 LLM API log，也不会发送给上游 provider。
- missing `tool_call_id` fail-fast，不再 fallback 生成 `tool_call_{index}`。
- response decode 保留 text、reasoning、tool calls、native metadata。

仍待拆分：

- `AgentModelGateway` 现在承担 encode/decode/sanitizer，后续应拆成 `agent_model_gateway/` 模块目录与 provider adapter 文件。
- 还没有正式 `ModelDelta` streaming adapter。
- 还没有 profile-driven provider switch policy。

## 4. 当前 Native Metadata 策略

Provider native data 是 opaque state，不是 Agent 业务语义。Runtime 可以携带和回放，但不能解释、改写或摘要。

已落地保留：

| Provider format | 保留字段 | 回放位置 |
| --- | --- | --- |
| Claude Messages | assistant `content` blocks，包含 `thinking` / `tool_use` / signature | Claude payload message conversion |
| Gemini | response `content.parts`，包含 `thoughtSignature` | Makersuite payload message conversion |
| Gemini Interactions | raw `outputs` | Gemini Interactions payload message conversion |
| OpenAI Responses | raw `output` items 与 `responseId` | Responses payload `input` items |

约束：

- tool call id 是不透明字符串。
- same-provider continuation 需要的 native state 丢失时必须 fail-fast 或测试失败。
- cross-provider switch 只能迁移 canonical 语义；旧 provider 的私有 signature/encrypted reasoning 不能伪装为目标 provider 可用状态。

## 5. 当前工具集

Tool registry 只产 canonical `AgentToolSpec`，不再暴露 OpenAI-shaped `openai_tools()`。

| Canonical name | Model alias | 类型 |
| --- | --- | --- |
| `chat.search` | `chat_search` | read-only |
| `chat.read_messages` | `chat_read_messages` | read-only |
| `worldinfo.read_activated` | `worldinfo_read_activated` | read-only |
| `workspace.list_files` | `workspace_list_files` | read-only |
| `workspace.read_file` | `workspace_read_file` | read-only |
| `workspace.write_file` | `workspace_write_file` | mutating |
| `workspace.apply_patch` | `workspace_apply_patch` | mutating |
| `workspace.finish` | `workspace_finish` | control |

当前尚未落地：

- `skill.list` / `skill.read`
- MCP 工具
- shell 工具
- 外部 extension tools
- tool approval / policy routing

## 6. 工具结果语义

必须区分两类错误：

- Recoverable tool error：模型参数、路径字符串、可见/可写策略、文件不存在、chat message index 不存在、读取范围非法、结果超过工具预算、patch 未完整读取、sha 过期、匹配 0 次或多次等模型可修正问题。返回 `AgentToolResult { is_error: true }`，写入 `tool_call_failed` warn event，并作为 tool message 回填下一轮模型。
- Fatal runtime error：journal 写入失败、workspace repository 内部 IO 错误、chat JSONL 损坏、manifest/checkpoint 损坏、模型响应结构不可解析、取消、序列化失败、状态机错误等宿主级问题。直接让 run 进入 failed 或 cancelled。

当前已落地 recent hydration：

- 前 5 轮 `workspace.write_file` / `workspace.apply_patch` 成功结果会读取目标 workspace 文件，将完整文本加入下一轮模型上下文。
- hydration 只影响 model request，不改变 workspace/journal 真相。
- hydration 会写 `context_tool_result_hydrated` debug event。

## 7. 当前运行流

```text
api.agent.startRunFromLegacyGenerate(input?)
  ↓
Legacy Generate dryRun 捕获 chatCompletionPayload 与 worldInfoActivation
  ↓
前端解析 chatRef / stableChatId
  ↓
start_agent_run(dto)
  ↓
AgentRuntimeService::start_run()
  ↓
创建 AgentRun / workspaceId / run workspace
  ↓
initialize_run 写 manifest / prompt snapshot / workspace root
  ↓
prepare_agent_tool_request 生成 AgentModelRequest
  ↓
model -> read-only context tools / workspace tools -> model -> ... -> workspace.finish
  ↓
工具调用参数与结果写入 workspace refs
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

## 8. 后续实施顺序

### 8.1 Gateway / Provider Contract 硬化

目标：把当前已落地的 gateway 核心拆成更长期可维护的 provider adapter 结构。

内容：

- 将 `agent_model_gateway.rs` 拆成模块目录：
  - `mod.rs`
  - `format.rs`
  - `encode.rs`
  - `decode.rs`
  - `schema.rs`
  - `providers/openai.rs`
  - `providers/responses.rs`
  - `providers/claude.rs`
  - `providers/gemini.rs`
- 增加 same-provider native metadata loss 测试。
- 增加 cross-provider switch policy 测试，明确哪些 metadata 不可迁移。
- 继续收紧 `provider_state` 契约测试，覆盖 Responses `messageCursor`、`previousResponseId`、session close 与日志剥离。
- 对齐 Responses WebSocket connector 与 HTTP client pool 的 proxy / timeout 语义，避免普通 Custom ChatCompletion 路径被 transport 细节隐性改变。
- 增加 schema sanitizer 覆盖更多 JSON Schema edge cases。

### 8.2 Profile 与 Context Policy

目标：让创作者控制模型、工具、预算和上下文，而不是写死在 runtime。

内容：

- profile 显式声明 allowed tools、tool budget、tool call mode。
- profile 显式声明 provider switch policy。
- profile 显式声明 ContextFrame 资源预算。
- Plan node 若锁定 profile，runtime 必须拒绝模型自行切换。

### 8.3 剩余只读上下文资源

目标：在不膨胀 prompt snapshot 的前提下，把创作者资源变成按需读取的工具/virtual resource。

优先级：

- `skill.list` / `skill.read`
- preset / character author resources 的统一 Skill-like 入口
- 可审计的 context budget 与 resource refs

### 8.4 Timeline UI 与人工控制

目标：给创作者可理解、可暂停、可提交的 Agent run 体验。

内容：

- Agent Mode toggle 与主发送按钮分流。
- 最小 timeline/event viewer。
- workspace artifact viewer。
- tool error / recovery 状态展示。
- commit preview 与手动提交。
- cancel UI。

### 8.5 Diff / Rollback / Resume

目标：让多轮创作具备可控回退能力。

内容：

- `readDiff()`：基于 checkpoint 对 workspace 文本文件生成 diff。
- `rollback()`：先只恢复 run workspace，不修改已提交聊天消息。
- `resumeRun()`：明确 run continuation 语义，避免复用已 closed run 的状态机。

### 8.6 MCP / Extension Tool

目标：引入外部工具生态，但保持 Tauri-native 安全边界。

约束：

- MCP Host ABI 独立于 Agent Mode。
- STDIO command/config 不得由 prompt、Preset、角色卡、世界书或第三方 JSON 任意写入。
- 危险工具必须进入 capability policy 与审批。
- Agent 消费 MCP tool 前必须经过 profile / policy resolution。

## 9. 验收命令

Agent 相关 Rust 变更至少运行：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml agent --lib
git diff --check
```

涉及 provider adapter / normalizer 时额外运行相关过滤测试：

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent_model_gateway
cargo test --manifest-path src-tauri/Cargo.toml openai_responses_payload
cargo test --manifest-path src-tauri/Cargo.toml claude_native_content_blocks_are_replayed
cargo test --manifest-path src-tauri/Cargo.toml normalize_
```

涉及前端 Host ABI、类型或契约时再运行：

```bash
pnpm run check:types
pnpm run check:frontend
pnpm run check:contracts
```

## 10. 每次修改必须同步的文档

- 当前事实：`docs/CurrentState/AgentFramework.md`
- 架构边界：`docs/AgentArchitecture.md`
- 硬契约：`docs/AgentContract.md`
- LLM gateway：`docs/Agent/LlmGateway.md`
- 工具语义：`docs/Agent/ToolSystem.md`
- workspace 语义：`docs/Agent/Workspace.md`
- 事件语义：`docs/Agent/RunEventJournal.md`
- 测试矩阵：`docs/Agent/TestingStrategy.md`
- Host ABI：`docs/API/Agent.md`、`docs/FrontendHostContract.md`、`src/types.d.ts`
