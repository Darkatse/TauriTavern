# Agent Prompt Assembly

本文档记录 2026-05-27 当前已落地的 Agent Profile 独立 Preset、独立 Model 与前后端提示词组装链路。历史推演见 `docs/PromptAssemblyPlan.md`；当前开发定位以本文为准。

## 核心原则

- 真实提示词组装仍由前端 SillyTavern `PromptManager` 完成，Rust 不重写近似版 prompt builder。
- Rust 负责解析 `ResolvedAgentProfile`、加载 preset、解析 LLM Connection，并生成前端 broker request。
- `FrozenRunInputSnapshot` 是本次 run 的输入事实；broker 只能从其中读取 `promptInputs`、`worldInfoActivation`、`macroContext`。
- `preset.ref` 是 prompt compiler input，不是对现有 `promptSnapshot` 的字符串补丁。
- `model.connectionRef + modelId` 与 preset provider source 解耦；preset 不拥有 endpoint、secret、最终 source/model。
- Profile 导出或嵌入到 Preset/Character 时必须移除本机 `connectionRef + modelId`，写为 `model.mode = "requiresConfiguration"`。
- 错误必须 fail-fast，不允许静默回退 Legacy Generate 或当前 UI preset/model。

## Profile 绑定语义

当前 Profile 相关字段：

```json
{
  "preset": {
    "mode": "ref",
    "ref": { "apiId": "openai", "name": "Writer Preset" }
  },
  "model": {
    "mode": "connectionRef",
    "connectionRef": "deepseek-main",
    "modelId": "deepseek-chat-v4-flash"
  }
}
```

`preset.mode`：

- `currentPromptSnapshot` / `none`：兼容路径，使用当前前端生成出的 prompt snapshot。
- `ref`：加载指定 OpenAI/chat-completion preset，并走 Frontend PromptAssemblyBroker。

`model.mode`：

- `currentPromptSnapshot`：沿用 prompt snapshot 中已有 source/model。
- `connectionRef`：通过 `LlmConnectionService` 解析 connection 与 `modelId`，覆盖组装 settings 与最终 runtime payload。
- `requiresConfiguration`：可保存、可导入、可展示的未配置模型状态；运行或 prompt assembly 前必须 fail-fast，用户需在本机重新选择模型后才能使用。

独立 preset 当前只支持 OpenAI/chat-completion preset，因为真实组装入口复用 `src/scripts/openai.js` 的 PromptManager 链路。

## FrozenRunInputSnapshot

前端在 Agent Mode 的 Legacy Generate 准备阶段冻结：

```json
{
  "schemaVersion": 1,
  "kind": "tauritavern.agentFrozenRunInputSnapshot",
  "generationType": "normal",
  "promptInputs": {},
  "worldInfoActivation": {},
  "macroContext": {}
}
```

来源：

- `promptInputs`：本次 Generate 已计算出的角色、世界书文本、extension prompts、bias、messages、messageExamples 等 PromptManager 输入。
- `worldInfoActivation`：本次 run 激活的世界书事实，用于 `worldinfo.read_activated` 与审计。
- `macroContext`：冻结 `{{user}}`、`{{char}}`、角色字段、persona、示例消息、`{{model}}` 等宏所需上下文。

`promptInputs.messages` 保存本次 run 的完整候选聊天历史输入，不在 Legacy Generate 阶段按 Agent Profile 提前裁剪。`context.initialChatHistoryMessages` 是 PromptManager 组装期的 Agent context policy：`-1` 表示不做显式楼数裁剪，`0` 表示初始 prompt 不注入真实聊天楼层，正数表示最多注入最近 N 楼；最终可进入 provider payload 的历史仍受 PromptManager token budget 限制。这样 runtime-time child/handoff 可以用同一份 frozen input 按 target Profile 重新组装自己的初始 prompt。

extension prompts 在冻结时会先执行 filter，只保留非空、结构化、可 clone 的 `value / position / depth / scan / role`。

## 当前生产链路

```text
用户点击发送
  -> sendTextareaMessage()
  -> getAgentGenerationOptions()
  -> Generate(..., agentMode=true)
  -> OpenAI 分支构造 promptInputs
  -> buildFrozenRunInputSnapshot()
  -> prepare_agent_prompt_assembly(dto)
  -> PromptAssemblyService 加载 preset.ref
  -> PromptAssemblyService 应用 model.connectionRef + modelId 到 settings
  -> 返回 frontendPromptAssembly broker request
  -> frontend buildPromptAssemblySnapshot(request)
  -> headless PromptManager 真实组装 messages
  -> createGenerationParameters(settings, model, ...)
  -> 得到 promptSnapshot.chatCompletionPayload
  -> start_agent_run(dto)
  -> AgentRuntimeService 提取 ChatCompletionGenerateRequestDto
  -> runtime 再次 apply_connection_to_payload()
  -> prepare_agent_tool_request() 注入 Agent tools
  -> AgentModelGateway -> ChatCompletionService -> provider request
```

注意：Rust 不是主动回调前端。root run 独立 preset 的实现是前端先调用 `prepare_agent_prompt_assembly`，Rust 返回 broker request，前端再本地调用 broker 组装并提交 `start_agent_run`。runtime-time child/handoff 组装则由 run event journal 发出轻量 `prompt_assembly_requested` 通知，前端 host bridge 用 `read_agent_prompt_assembly_request` 按需读取 pending broker request，再调用 `resolve_agent_prompt_assembly` 回填结果。

## Broker Request

Rust 返回的 `frontendPromptAssembly` request 包含：

```text
schemaVersion
kind = tauritavern.agentPromptAssemblyRequest
assemblyId?        # invocation-scoped runtime request only
scope?             # run/invocation/task metadata for runtime request only
profileId
generationType
frozenRunInputSnapshot
settings
modelId?
presetRef
agentContextPolicy
agentSystemPrompt
agentTaskPrompt?   # child/handoff task packet materialized through agentTask marker
requiredAgentPromptComponents?
jsonSchema?
fingerprint { presetSha256, frozenRunInputSnapshotSha256, agentTaskPromptSha256? }
```

`settings` 是“preset settings + model binding overlay”后的有效 settings。broker 不允许额外接收顶层 `promptInputs`、`worldInfoActivation`、`macroContext`，防止冻结输入事实分叉。

runtime-time `prompt_assembly_requested` event 不携带完整 request，只包含 `assemblyId`、`scope`、`requestKind`、`requestSchemaVersion` 与 `requestFingerprint`。完整 request 存在 runtime pending map 中，前端 bridge 只能在该 assembly 仍 pending 时读取。这样 event journal 保持轻量，低端移动端轮询不会反复搬运 frozen input、settings 与 prompt 文本。

## Model 解耦与双阶段覆盖

第一阶段发生在 `PromptAssemblyService`：

1. 加载 `preset.ref` 的原始 preset settings。
2. 若 `model.mode = connectionRef`，解析 connection 和 `modelId`。
3. 删除 preset 中 connection-owned fields：`chat_completion_source`、各 source model key、`custom_url`、`secret_id`、reverse proxy、source-specific endpoint 等。
4. 写入 broker 组装所需的 `chat_completion_source` 和对应 source model key，例如 DeepSeek 写 `deepseek_model`。

第二阶段发生在 Agent runtime：

1. `start_agent_run` 收到 broker 产出的 `promptSnapshot.chatCompletionPayload`。
2. executor 在 tool loop 前调用 `apply_connection_to_payload(connectionRef, modelId, payload)`。
3. 最终 payload 的 source、model、endpoint、secret、reverse proxy、adapter hints 以 LLM Connection 为权威。

这两个阶段都需要存在：前端组装阶段需要正确 source/model 计算 PromptManager 与 generation parameters；runtime 阶段需要保证真正发送请求时不受 preset 中旧连接信息污染。

## 关键文件

- `src/script.js`：Agent 发送入口、Legacy Generate 准备、`FrozenRunInputSnapshot` 创建。
- `src/scripts/tauritavern/agent/frozen-run-input-snapshot.js`：冻结输入结构与 normalization。
- `src/tauri/main/api/agent-prompt-assembly-run.js`：prepare/buildSnapshot 编排。
- `src/tauri/main/api/agent-prompt-assembly.js`：Frontend PromptAssemblyBroker。
- `src/tauri/main/api/agent-prompt-assembly-bridge.js`：runtime-time child invocation prompt assembly host bridge，按 `assemblyId` 读取 pending broker request。
- `src/scripts/openai.js`：headless PromptManager、settings normalization、真实 OpenAI/chat-completion prompt assembly。
- `src-tauri/src/application/services/prompt_assembly_service.rs`：Rust PromptAssemblyService、preset 加载、broker request、组装阶段 model overlay。
- `src-tauri/src/application/services/agent_runtime_service/prompt_assembly.rs`：invocation-scoped prompt assembly request/resolve、snapshot 持久化。
- `src-tauri/src/application/services/llm_connection_service.rs`：runtime payload 连接覆盖。
- `src-tauri/src/application/services/agent_runtime_service/lifecycle.rs`：`start_agent_run` 输入校验与 run 创建。
- `src-tauri/src/application/services/agent_runtime_service/executor.rs`：runtime model binding、tool request 准备。
- `src-tauri/src/application/services/agent_model_gateway/`：Agent canonical IR 与 ChatCompletion DTO 转换。

## 兼容边界

- 不切换当前 UI preset，不修改 global `oai_settings`，不污染当前前端模型选择。
- headless broker 默认不触发普通 `CHAT_COMPLETION_PROMPT_READY`；依赖该事件改写 prompt 的扩展不会参与 Agent 独立组装。
- 动态 extension prompts 在 frozen snapshot 创建时已经物化；broker 组装阶段不会重新执行动态 filter。
- Agent runtime 拒绝外部 `tools`、`tool_choice`、已有 `role: "tool"` 或 assistant `tool_calls`。
- return-mode child invocation 在 target Profile 使用 `preset.ref` 时走 runtime-time PromptAssemblyBroker handshake；`currentPromptSnapshot` / `none` Profile 保持后端兼容组装路径。

## 后续开发注意

- 不要把独立 preset 实现为临时切换 UI preset 后 dryRun。
- 不要在 Rust 中手写简化 prompt builder 来替代 PromptManager。
- 新增 provider source 时，需要同步 `PromptAssemblyService::prompt_model_setting_key` 与 LLM connection payload overlay。
- 后续 handoff 应复用 `FrozenRunInputSnapshot`、`agentTask` marker 与 invocation-scoped broker contract；若要扩展 task/handoff packet，优先扩展 broker request 的 `scope` 与 task prompt 渲染，而不是在 Rust 中手写 PromptManager 替代品。
