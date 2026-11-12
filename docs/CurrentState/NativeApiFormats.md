# 原生模型 API 格式

本文说明聊天请求如何从前端的 OpenAI 兼容形状转换为各家原生协议，以及响应如何归一化后交还前端。Agent 的续接状态见 [LLM gateway](../Agent/LlmGateway.md)。

## 1. 格式与链路

前端提交 OpenAI 兼容的 generate payload。Rust payload builder 按格式构造上游请求，provider repository 负责传输，并把响应归一化为 `chat.completion` 与 `chat.completion.chunk`。前端按格式选择解析分支，并在消息的 `extra.native` 中保存原生内容。

| 格式 | 上游端点 | 交给前端的流 | 原生内容 |
| --- | --- | --- | --- |
| OpenAI 兼容 | `/chat/completions` | chunk | `tool_calls[].extra_content` |
| OpenAI Responses | `/responses` | 归一化为 chunk | `openai_responses.output` |
| Claude Messages | `/messages` | Anthropic 事件 | `claude.content`（仅含 client `tool_use` 的回合） |
| Gemini generateContent | `/models/{model}:generateContent` 与 `:streamGenerateContent` | Gemini 事件，末包带原生内容 | `gemini.content` |
| Gemini Interactions | `/interactions` | 归一化为 chunk，末包带原生内容 | `gemini_interactions.steps` |

格式由 `ChatCompletionProviderFormat` 统一判定：Custom 与 OpenCode 取连接设置，内建来源取来源与模型。例如原生 OpenAI 的 GPT-6 使用 Responses，直连、Bedrock 与 Vertex 上的 Claude 使用 Claude Messages。Custom 的 repository 再按 endpoint path 选择传输。

响应中明确的 error（包括 HTTP 200 的 JSON error）在共同读取处转为错误，Legacy 与 Agent 走同一传播路径。流式前端只跳过非 JSON 帧。

normalizer 把缓存读取数写入 `usage.prompt_tokens_details.cached_tokens`，缺失时不补零；Claude 的 `prompt_tokens` 包含缓存读取、缓存写入与未缓存输入。前端把缺失或不一致的计数视为未知，此时不显示缓存命中率。OpenAI 兼容的流式请求设置 `stream_options.include_usage=true`，usage 尾包可以没有 choices，读取要持续到流正常结束。

## 2. 请求参数

Reasoning Effort 选 `Auto` 时不发送 `reasoning_effort`。

内建来源按已核实的模型事实构造请求。这些事实集中在 `model_capabilities.rs` 与各来源的 builder，本文不逐个列举模型；它们只用于构造请求，不在本地重复服务商的参数校验。共同规则如下：

- 推理强度换算为模型支持的档位。只有 low、high、max 三档的模型共用一套映射：Minimum 与 Low 发 low，Medium、High 与 XHigh 发 high，Maximum 发 max。
- 模型固定的采样参数，以及当前推理强度下不接受的采样参数，不发送。
- “请求思维链”对能开关思考的模型决定是否思考。始终思考的模型不发送思考开关，此时该设置只决定是否保留思考内容，即是否显示、是否在工具续轮中回传。
- 不接受强制工具调用的 Claude 模型，JSON schema 使用 `output_config.format`；其他 Claude 使用 schema tool。

Custom 连接不按模型名启用、校验或降级参数。显式值按格式转换后发送：`reasoning_effort` 对应 Responses 的 `reasoning.effort`、Claude 的 `output_config.effort` 与 Interactions 的 `generation_config.thinking_level`。上游不接受时错误原样返回，用户可以用 include/exclude body override 覆盖或删除字段。以下两处按模型名处理：

- Gemini generateContent 的模型名中能识别出 Gemini 模型 ID 时（包括 `models/` 与中转前缀），推理强度按该模型的思考级别或预算映射；其他别名映射为 `thinkingLevel`。
- Custom 与 OpenCode 的 OpenAI 兼容格式下的 DeepSeek V4 与原生 DeepSeek 共用后端适配，在协议构建之后、用户覆盖之前执行。

## 3. 工具回合与原生内容

- `tool_call_id` 是不透明字符串，原样传递与存储。
- 调用与结果的配对由服务商判定。两种 Gemini 格式需要工具结果的名称：先取它所回应的调用的名称，再取消息的 `name`，都没有时报错。
- 消息的 `name` 在 user 与 assistant 上是说话人，Prompt Post-Processing 与 Cohere 把它写成正文前缀后删除；在 tool 消息上是工具名，不并入正文，后处理让它与 `tool_call_id` 一起保留或删除。
- 工具定义的 `parameters` 是 JSON Schema。构造器把它写入本格式的 schema 字段，除个别来源已核实的约束外不做改写。function 的其余字段只有 OpenAI 兼容格式原样发送，其他格式只取自己支持的字段，例如 Responses 与 Claude 的 `strict`。
- `native` 与 `signature` 是 TT 的扩展字段。`build_payload` 在 Prompt Post-Processing 与格式构造之前统一投影：`native` 只保留本次格式的键，不是对象时报错；`signature` 只保留给两种 Gemini 格式与 OpenRouter。
- OpenRouter 只从 `reasoning.encrypted` 类型的 `reasoning_details` 项读取签名，构造器把 `signature` 改写为这类项。
- 原生内容原样保存，不清洗未知字段。回放前由 `provider-replay.js` 核对正文、来源、模型与格式。Legacy 回放 Claude 与两种 Custom Gemini 格式的原生内容。
- SillyTavern 会把一次工具回合拆成相邻的可见消息与 invocation 消息。两份原生内容完全相同时只回放一次；Claude 在两者不一致时报错，编辑其中任一条会使两份同时失效。
- `extra_content` 由服务商定义，流式时以最新的非 null 快照整体替换。Legacy 与 Agent 按原 tool call 位置保存，切换 provider 或模型后不携带旧值。
- Legacy 工具续轮在开启“请求思维链”、模型与说话角色不变时，随 assistant tool call 消息回传该轮的 `reasoning_content`。使用这一字段的连接是 DeepSeek、Moonshot、Z.AI、MiniMax 原生渠道，以及兼容格式下的 DeepSeek V4。OpenRouter 与其余 Custom 连接按 `tool_reasoning_mode` 转发 interleaved `reasoning`。一个连接只使用其中一种。
- Moonshot 的 partial 模式只从 `reasoning_content` 接续思考。隐藏预填（最后一条 assistant 消息）若以推理模板的推理块开头，前端把推理块移入 `reasoning_content`，未闭合的推理块也是如此。`Start Reply With` 与「继续」的消息属于回复正文，推理由前端从回复文本中解析，保持文本形式。
- OpenAI Responses 默认重放完整消息，Agent 还回放原生输出；`previous_response_id` 只在 WebSocket 模式下使用，见 [续接状态](../Agent/LlmGateway.md#续接状态)。

## 4. 各格式要点

### OpenAI Responses

- assistant `tool_calls` 编码为 `function_call`，tool 消息编码为 `function_call_output`；缺少 id、名称、参数或 `tool_call_id` 时报错。
- `store` 为 `false`，`include` 包含 `reasoning.encrypted_content`。`json_schema` 映射为 `text.format`。`enable_web_search` 加入托管的 `web_search` 工具，它不进入 Agent 本地工具表。
- 默认非流式请求走 HTTP，流式请求走 SSE。连接设置中的 WebSocket 模式默认关闭；开启后 WebSocket、`previous_response_id` 与增量输入一起生效，失败时不回退 HTTP。只有内部 `_tauritavern_provider_state.transport` 为 `responses_websocket` 的请求使用 run 级持久会话。
- WebSocket 经 `HttpClientPool` 的 ChatCompletion WebSocket profile 建连，沿用代理、TLS 与超时设置；会话键包含 transport revision，代理或 client 配置变化后重建会话。
- 上游 payload 移除 `_tauritavern_provider_state`；WebSocket 的 `response.create` 还移除 `stream` 与 `background`。
- 流式语义事件归一化为 chunk。完成的 function call 作为单个 `tool_calls` delta 发出，`id` 取 `call_id`。流必须以 terminal event 结束，提前断开时报错，用户取消除外。

### Gemini Interactions

- `custom_url` 不以 `/v1` 或 `/v1beta` 结尾时补 `/v1beta`。用户提供 `Authorization` header 时使用它，否则使用 `x-goog-api-key`。
- `system` 消息合并为 `system_instruction`；JSON schema 写入 `response_format`。
- 状态事件同时接受现网的 `interaction.status_update` 与文档中的命名，只以终态事件完成响应。
- `thought_summary` 映射为 `reasoning_content`，`thought_signature` 只保留在原生 step 中。function call 参数累积到 `step.stop` 后解析，只发出一个完整的 tool call。服务端工具 step 与 annotations 只保留在原生内容中。
- 流式 function call 的终态可以是 `completed`，因此 `finish_reason=tool_calls` 由组装出的 function call step 决定。`incomplete` 有可用文本时归一化为 `length`，否则报错。

### Claude Messages

- Custom Claude Messages 默认不发送 `anthropic-beta`。显式开启 prompt caching 或快速模式时补充对应 header；prompt caching 还需满足 TT 的缓存策略。
- `enable_web_search` 在直连与 Custom 中加入 `web_search_20250305` 工具。`server_tool_use` 与 `web_search_tool_result` 留在原生内容中，不投影为本地工具调用。Vertex 与 Bedrock 上的 Claude 不使用该设置。
- 图片：data URL 转为 base64 source；直连与 Custom 的远端 URL 转为 url source。Bedrock 只接受 base64，无法保真的输入报错。`input_image.file_id` 不转换为 Files API 引用。
- 后端透传 Anthropic 事件，前端 Claude 分支按 content block index 累积各类 delta。`stop_reason` 为 `refusal` 时保留输出并提示；为 `max_tokens` 或 `model_context_window_exceeded` 时保留部分文本并提示截断。这些终态都不执行未完成的 tool call。

### Gemini generateContent

- Custom 复用 Google AI Studio 的翻译与传输。Base URL 与模型分开配置：保留显式的 `/v1` 或 `/v1beta`，否则补 `/v1beta`。Additional Parameters 在翻译之后应用。`n` 多候选尚未实现。
- 工具定义的 schema 写入 `parametersJsonSchema`，`json_schema` 写入 `generationConfig.responseJsonSchema`。`parameters` 与 `responseSchema` 只接受 OpenAPI 子集，且 Gemini API 与 Vertex 接受的范围不同。payload 中直接写的 `responseSchema` 原样使用。
- 普通消息的 `native.gemini.content.parts` 原样进入 `contents`。无法核对的旧记录只保留正文与可见推理；流式请求完整结束后才提交原生内容。
- Google AI Studio、Custom 与 Vertex AI 共用流式累加与响应归一化。响应须包含可见文本、工具调用或内联媒体；没有候选时报告 `promptFeedback.blockReason`。
- 模型列表读取全部分页并保留 metadata，中途失败时不返回部分列表。服务器返回已经请求过的分页令牌时停止分页，丢弃重复的一页。

## 5. 设置与回滚

Custom 变体在设置中保存为 `chat_completion_source="custom"`，格式记在 `custom_api_format`。Connection Profile 同样记录 `api=custom`，格式由 `custom-api-format` 字段回放。这样设置拷回 SillyTavern 1.16.0 后仍能启动，未知字段被忽略。

模型列表与状态检查按格式选择接口：OpenAI 兼容与 Responses 使用 `/models`，Claude Messages 使用 Claude `/models`，两种 Gemini 格式使用 Gemini `/models`。端点预览按所选格式推导，地址规则与后端一致。

## 修改入口

- 格式判定：[`exchange.rs`](../../src-tauri/crates/tt-application/src/services/chat_completion_service/exchange.rs)
- 请求构建：[`payload/`](../../src-tauri/crates/tt-application/src/services/chat_completion_service/payload)，扩展字段投影在 `extension_fields.rs`，工具调用与结果的共用规则在 `tool_calls.rs`，Custom 分流在 `custom.rs`，模型事实在 [`model_capabilities.rs`](../../src-tauri/crates/tt-application/src/services/chat_completion_service/model_capabilities.rs)
- 传输、流式与归一化：[`http_chat_completion_repository/`](../../src-tauri/crates/tt-adapter-provider-http/src/http_chat_completion_repository)，按 endpoint 分流在 `mod.rs`，响应归一化在 `normalizers.rs`
- 前端：[`openai.js`](../../src/scripts/openai.js) 负责格式选择、流式解析与工具回合组装，[`provider-replay.js`](../../src/scripts/tauritavern/provider-replay.js) 负责回放核对
