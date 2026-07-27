# 原生 API 格式（Custom）兼容现状

最后更新：2026-07-27

本文件描述 **TauriTavern 已落地** 的三家原生 API 格式兼容（OpenAI Responses / Claude Messages / Gemini Interactions）的当前实现快照与持续开发约束。

目标边界（回滚兼容）：
- “倒回 SillyTavern”后，ST 1.16.0 **能启动且设置不崩** 即可（不追求 ST 原生理解新字段）。

---

## 1. 当前解决了什么问题

在保持前端尽量沿用 SillyTavern 语义（`chat.completion` / tool loop / 事件流）前提下，为 `Custom` 入口新增三种“原生协议”变体：

- **OpenAI Responses**：`/v1/responses`（支持 stream + tool calling）
- **Claude Messages**：`/v1/messages`（Custom 变体默认不注入 `anthropic-beta`；仅在用户显式启用 Claude prompt caching 时自动补充 caching 所需 header）
- **Gemini Interactions**：`/v1beta/interactions`（支持 stream + tool calling + thought signature/native blocks 回放）

核心原则：
- **协议复杂度集中在 Rust 后端 translator + normalizer**；前端尽量只做最小选择/回显与少量解析分支。
- **配置回滚友好**：落盘仍以 `chat_completion_source=custom` 为主，新能力通过新增字段 `custom_api_format` 扩展。

---

## 2. 端到端链路现在如何工作

### 2.1 配置与选择（前端）

UI：OpenAI 设置的 `Chat Completion Source` 增加 3 个选项：
- `Custom (OpenAI Responses)`
- `Custom (Claude Messages)`
- `Custom (Gemini Interactions)`

落盘语义（关键契约）：
- 任何 “Custom (*)” 变体最终都落到：
  - `oai_settings.chat_completion_source = "custom"`
  - `oai_settings.custom_api_format ∈ {"openai_compat","openai_responses","claude_messages","gemini_interactions"}`

这保证把配置文件拷回 ST 1.16.0 时：
- `chat_completion_source` 仍是 ST 已知的 `custom`
- `custom_api_format` 作为“未知字段”被 ST 忽略（但不应导致设置页崩溃）

Connection Profiles（Connection Manager 扩展）：
- profile 中的 `api` 对 Custom 统一记录为 `custom`，避免把 UI 变体值写入配置造成回滚风险。
- Custom 变体由单独字段 `custom-api-format` 记录与回放（等价于执行 `/custom-api-format <format>`）。

自定义端点预览（UI 文案）：
- 端点预览只展示 **当前所选格式** 的最终 endpoint（base URL + suffix），并保留“末尾加 `/v1` 试试”的提示。
- suffix 映射：OpenAI-compatible→`/chat/completions`，Responses→`/responses`，Claude→`/messages`，Gemini→`/interactions`。

### 2.2 请求构建（Rust payload builder）

后端入口仍按“OpenAI 兼容 generate payload”接收前端数据，在 `payload/custom.rs` 中按 `custom_api_format` 分流：
- `openai_compat` → 走现有 `/chat/completions` 兼容构造，并应用 include/exclude overrides
- `openai_responses` → 构造 `/responses`
- `claude_messages` → 复用 Claude Messages 构造，并应用 include/exclude overrides
- `gemini_interactions` → 构造 `/interactions`

### 2.3 HTTP 调用 + Stream 处理（Rust repository）

仓库层对 `ChatCompletionSource::Custom` 以 **endpoint_path** 再分流（`http_chat_completion_repository/mod.rs`）：
- `/responses` → OpenAI Responses repository（语义 SSE → 归一化 chunk）
- `/interactions` → Gemini Interactions repository（语义 SSE → 归一化 chunk）
- `/messages` → Claude repository（沿用 Claude 的事件流语义）
- 其他 → Custom OpenAI-compatible（`/chat/completions`）

> 备注：Claude 的 streaming 仍保持“Anthropic 事件流 JSON”语义；Responses/Interactions streaming 则统一归一化为 OpenAI `chat.completion.chunk`。

---

## 3. 已支持能力 / 明确不支持

### 3.1 能力矩阵（当前）

| Custom 变体 | 非流式 | 流式 | tool calling | thought signature / native blocks | 回滚 ST 启动 |
|---|---:|---:|---:|---:|---:|
| OpenAI-compatible (`/chat/completions`) | ✅ | ✅ | ✅（上游 ST 语义） | ✅（现有链路） | ✅ |
| OpenAI Responses (`/responses`) | ✅（normalize→chat.completion） | ✅（Responses events→chat.completion.chunk） | ✅（full transcript replay / `previous_response_id`） | ✅（backend normalizer / Agent gateway 保留 raw `output` 与 `responseId`） | ✅ |
| Claude Messages (`/messages`) | ✅（normalize→chat.completion） | ✅（Anthropic events） | ✅（沿用 Claude tool loop） | ✅（现有链路） | ✅ |
| Gemini Interactions (`/interactions`) | ✅（normalize→chat.completion，含 native） | ✅（SSE→chat.completion.chunk，末包带 native） | ✅ | ✅（`message.extra.native` 回放 steps） | ✅ |

### 3.2 明确的当前限制

- **Custom OpenAI Responses 不再维护 call_id → response_id 内存缓存**。普通 Custom 请求依赖完整 transcript / native output replay；带 `previous_response_id` 的请求允许只发送新的 function call outputs。Agent 请求的 `previous_response_id` 来自 run-scoped `provider_state`。
- **Custom 的 model list / status check** 已按 `custom_api_format` 对齐传输协议：OpenAI-compatible / Responses 继续使用兼容 `/models`，Claude Messages 使用 Claude `/models`，Gemini Interactions 使用 Gemini `/models`。
- **Claude streaming 不做 chunk 归一化**：前端需走 Anthropic events 分支解析（现状就是如此，优先复用既有 Claude 语义）。

---

## 4. 三家实现要点（对持续开发最关键的部分）

### 4.1 OpenAI Responses（/responses）

请求侧（payload）：
- `messages[]` → `input[]` items；`system` → `developer`
- assistant message 若携带 `message.native.openai_responses.output`，则原样回放 raw Responses `output` items，并记住其中的 `function_call.call_id`
- assistant text 会编码为 Responses `message` / `output_text`
- assistant `tool_calls[]` 会编码为 Responses `function_call` items；`id` / `function.name` / `function.arguments` 必须可解析，缺失结构会 fail-fast
- `tool` / `function` message 会编码为 `function_call_output`，必须有 `tool_call_id`
- 没有 `previous_response_id` 时，`function_call_output` 必须能在同次 transcript 中找到前置 `function_call`；否则 fail-fast
- 有 `previous_response_id` 时，允许 orphan `function_call_output`，因为前置 function call 可由 provider previous response state 提供
- `store` 默认 `false`；`include` 会保证包含 `reasoning.encrypted_content`，用于 reasoning/native continuation
- `previous_response_id`、`max_tokens` / `max_completion_tokens`→`max_output_tokens`、`reasoning_effort`→`reasoning.effort`、`verbosity`→`text.verbosity`、`metadata`、`parallel_tool_calls` 等字段按当前 payload builder 映射

传输侧（repository）：
- 普通 Custom `/responses` 非流式请求走 HTTP，流式请求走 SSE
- 带内部 `_tauritavern_provider_state.sessionId` 的请求走 run-scoped persistent WebSocket session；该路径失败时不回退 HTTP
- Responses WebSocket 建连通过 `HttpClientPool` 的 ChatCompletion WebSocket profile 发起 HTTP Upgrade，再交给 WebSocket frame stream；因此沿用现有代理、TLS/client 构建与连接超时契约
- persistent session 的 connection key 包含 transport revision；request proxy / client 配置变更后会重建 session
- 上游 HTTP payload 会剥离 `_tauritavern_provider_state`
- WebSocket `response.create` payload 会剥离 `_tauritavern_provider_state`、`stream` 与 `background`

流式侧（repository）：
- 解析 Responses 语义事件（如 `response.output_text.delta` / `response.refusal.delta` / `response.output_item.done`）
- 输出 OpenAI `chat.completion.chunk`：
  - 文本 delta → `choices[0].delta.content`
  - 推理 delta → `choices[0].delta.reasoning_content`
  - 完成的 function call item → 单个 `choices[0].delta.tool_calls[]`（`id` 使用 Responses 的 `call_id`）
- SSE 必须以 Responses terminal event 结束；连接提前关闭会 fail-fast，用户主动取消除外

tool follow-up（关键契约）：
- 普通 Custom Responses 不再依赖 repository 内存缓存。若没有 `previous_response_id`，请求必须通过 full transcript replay 或 native output replay 提供前置 `function_call`。
- 若 payload 已有 `previous_response_id`，builder 允许只发送对应的 `function_call_output`。
- Agent Responses follow-up 由 `AgentModelGateway` 的 `provider_state.previousResponseId` 驱动；详见 `docs/CurrentState/AgentProviderState.md`。

### 4.2 Gemini Interactions（/v1beta/interactions）

URL 与鉴权：
- 若 `custom_url` 末尾不含 `/v1` 或 `/v1beta`，后端自动补 `.../v1beta`
- 用户显式提供 `Authorization` 时优先使用该 header；否则使用 `x-goog-api-key`
- streaming 由请求体 `stream: true` 启用，响应按 SSE 解析

请求侧：
- `system` message 聚合为顶层 `system_instruction`
- user / assistant / tool history 分别构造 `user_input`、`model_output` / `function_call`、`function_result` steps
- structured output 使用 `response_format = { "type": "text", "mime_type": "application/json", "schema": ... }`

signature / native blocks（关键契约）：
- 后端在 streaming 完成事件 `interaction.completed` 时，将聚合后的 `steps[]` 放入：
  - `choices[0].delta.native = { gemini_interactions: { steps } }`
- 前端在保存消息时将其落到 `message.extra.native`
- 后续构造 stateless history 时：若 `extra.native.gemini_interactions.steps` 存在，则 **原样回放** steps（满足 thought-signatures 相关要求）
- SillyTavern 将带前导文本的 function-call turn 拆成相邻的可见消息与 tool invocation 时，payload translator 只对两者完全相同的 native steps 去重并回放一次

流式归一化：
- Interactions SSE 顺序为 `interaction.created` / 状态通知，随后每个输出依次经历 `step.start` / `step.delta` / `step.stop`，最后是 `interaction.completed` 与 `[DONE]`；适配器接受现网的 `interaction.status_update` 与文档命名的状态事件，但只以终态事件完成响应
- `step.delta.type=text` → `delta.content`
- `step.delta.type=thought_summary` → `delta.reasoning_content`；`thought_signature` 只保留在 native step
- function call 的 id / name 来自 `step.start.step`；`step.delta.type=arguments_delta` 的 `arguments` 字符串累计到 `step.stop` 后解析，并只发送一个完整 `delta.tool_calls` item
- Google Search 等服务端工具 step 与 text annotations 不投影到通用 chat 字段，只在流式组装后原样保留于 native steps
- streaming function call 的 terminal status 可以是 `completed`，因此 `finish_reason=tool_calls` 由已组装的 `function_call` step 决定；非流式 function call 的状态可以是 `requires_action`
- `incomplete` 若含可消费文本则归一化为 `finish_reason=length`；无可消费输出或不完整 function call 直接报错

### 4.3 Claude Messages（/messages，Custom 变体）

header 策略（关键契约）：
- **Custom Claude Messages 默认不自动添加 `anthropic-beta`**，避免第三方兼容端报错。
- 当前新增显式 opt-in：只有当用户为 `custom_api_format=claude_messages` 勾选“Apply Claude Prompt Caching Strategy”且 TT 的 Claude Prompt Cache 未关闭时，后端才会：
  - 复用 Claude prompt caching 断点策略
  - 为请求自动补充 prompt caching 所需的 `anthropic-beta` caching header
- 未勾选时，仍保持“仅透传用户自定义 headers”的兼容策略。

image 输入：
- Claude Messages 复用 shared `content_parts` parser：`image_url` data URL 转成 `source.type=base64`，direct/custom Claude Messages 的远端 `http(s)` URL 转成 `source.type=url`。
- AWS Bedrock Claude 复用 Claude renderer，但执行 base64-only source policy；远端 URL、provider file reference 等不能保真的输入会 fail-fast。
- OpenAI-style `input_image.file_id` 暂不自动转成 Claude Files API 引用；Claude-native file source 只按 native block 保真回放，调用方需自行负责 beta/header/file 生命周期。

streaming 语义：
- 后端沿用 Claude 的 SSE `data:` JSON 事件透传（不做 chunk 归一化）
- 前端对 direct Claude、Vertex Claude、内建 Bedrock Claude 与 `custom_api_format=claude_messages` 走 Claude streaming 分支解析
- 前端按 content block index 累积 `text_delta`、`thinking_delta`、`signature_delta` 与 `input_json_delta`；`input_json_delta` 按 delta 契约处理，不绑定具体 tool block type
- 前端在 `message_delta` / 非流式响应的 `stop_reason` 上显式处理终态：`refusal` 保留 provider 输出、显示 toast，并将同一警告追加到最终 `message.mes`；`max_tokens` / `model_context_window_exceeded` 保留部分文本、显示截断警告；这些终态都不会执行或回放未完成的 tool call
- 只有包含 client `tool_use` 的 assistant turn 才把完整 `content[]` 保存到 `message.extra.native.claude` 并在同 provider/model 的后续请求原样回放；普通 assistant turn 继续使用 SillyTavern canonical content，避免历史 thinking 绕过 token budget 与消息编辑语义
- SillyTavern 将一次 tool turn 拆成相邻可见消息与 invocation 消息时，translator 仅在两者 native content 完全相等时折叠为一次，内容不一致则 fail-fast；编辑其中任一消息会同时使两份 native metadata 失效

---

## 5. 最容易误改的契约（请勿破坏）

1. **回滚兼容**：Custom 变体落盘必须保持 `chat_completion_source="custom"`，不要把 UI 选择值（如 `custom_openai_responses`）写入设置文件。
2. **tool_call_id 透明性**：tool loop 不应假设 tool_call_id 是 OpenAI UUID；必须把它当作不透明字符串传递与存储。
3. **native metadata 保真**：Agent gateway 会通过 normalized `message.native` / canonical `Native` part 保留 Claude content blocks、Gemini content parts、OpenAI Responses output items、Gemini Interactions steps。不得“清洗未知字段”，否则签名链或 reasoning continuation 会断。
4. **Custom Claude 不注入 anthropic-beta**：该行为是为了兼容第三方；现在只有显式 opt-in 的 prompt caching 会自动补 caching header，其他场景仍不得硬编码回退。
