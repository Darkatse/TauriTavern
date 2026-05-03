# TauriTavern Agent LLM Gateway

本文档定义 Agent Runtime 与现有 LLM 调用链之间的边界。

结论：Agent 内部使用 provider-agnostic canonical model IR；provider 边界继续复用现有 `ChatCompletionService`；provider-native metadata 作为 opaque state 保留和回放，不成为 Agent 业务语义。

## 1. Ground of Truth

当前后端 LLM 事实：

- Provider source 定义在 `ChatCompletionSource`。见 `src-tauri/src/domain/repositories/chat_completion_repository.rs`。
- Payload builder 由 `application/services/chat_completion_service/payload/mod.rs` 按 provider 分发。
- `ChatCompletionService` 负责 source 解析、iOS policy、endpoint override、feature policy、settings、secret、prompt caching、payload build、generate/generate_stream/cancel。
- LLM API 日志依赖 `LoggingChatCompletionRepository` wrapper。
- Custom Native API 文档强调 tool call id 透明性与 native metadata 保真。见 `docs/CurrentState/NativeApiFormats.md`。

Agent 不维护第二套 provider registry，不直接调用 `HttpChatCompletionRepository`，不自行读取 secret，不绕过现有 policy/logging/prompt cache。

## 2. 当前调用链

当前调用链：

```text
AgentRuntimeService
  -> AgentModelGateway.generate_with_cancel(AgentModelRequest)
    -> encode_chat_completion_request()
      -> ChatCompletionService.generate_exchange_with_cancel(ChatCompletionGenerateRequestDto)
        -> payload builder
        -> ChatCompletionRepository
        -> LoggingChatCompletionRepository
        -> HttpChatCompletionRepository
    -> decode_chat_completion_response()
  -> AgentModelResponse
```

当前只实现非 streaming Agent tool loop。`ChatCompletionStreamEvent::Chunk` 仍只是 provider SSE bridge，不是 Agent timeline event。

当前代码布局：

- `agent_model_gateway/mod.rs`：`AgentModelGateway` trait、`AgentModelExchange`、`ChatCompletionAgentModelGateway` wrapper。
- `encode.rs` / `decode.rs`：canonical `AgentModelRequest` / `AgentModelResponse` 与 normalized ChatCompletion exchange 的双向转换。
- `format.rs`：根据 request payload 解析 `ChatCompletionSource` 与 `ChatCompletionProviderFormat`。
- `schema.rs`：按 provider adapter 清洗 tool JSON Schema。
- `provider_state.rs`：写入内部 `_tauritavern_provider_state`，并生成下一轮 run-scoped continuation。
- `providers/*`：OpenAI-compatible、OpenAI Responses、Claude、Gemini / Gemini Interactions 的 provider-specific 规则。

## 3. Canonical Model IR

Agent runtime 只消费 canonical IR：

```rust
AgentModelRequest {
    payload,
    messages,
    tools,
    tool_choice,
    provider_state,
}

AgentModelResponse {
    message,
    tool_calls,
    text,
    provider_metadata,
    raw_response,
}
```

`AgentModelContentPart` 当前支持：

```text
Text
Reasoning
ToolCall
ToolResult
Media
ResourceRef
Native
```

设计原则：

- `Text` / `ToolCall` / `ToolResult` 是可迁移语义。
- `Reasoning` 可以保存 provider 返回的可见/摘要化 reasoning，但不要求所有 provider 都有。
- `Native` 保存 provider-private blocks，不解析、不清洗、不改写。
- `payload` 仍承载现有 ChatCompletionService 需要的 source/model/settings 字段。
- `provider_state` 是当前已落地的 run-scoped continuation state，详见 `docs/CurrentState/AgentProviderState.md`。

## 4. Provider Format

当前 gateway 根据 request payload 判断 provider format：

- OpenAI-compatible
- OpenAI Responses
- Claude Messages
- Gemini / Makersuite / Vertex AI
- Gemini Interactions

转换方向：

```text
canonical AgentModelRequest
  -> OpenAI-compatible messages/tools shape
  -> existing provider payload builder
  -> provider native payload
```

响应方向：

```text
provider native response
  -> existing normalizer / repository
  -> OpenAI-compatible normalized response with message.native
  -> AgentModelResponse
```

这个过渡结构保留了现有 ChatCompletionService 投资，同时让 Agent runtime 摆脱 OpenAI-shaped raw JSON。

## 5. Native Metadata Contract

Provider native metadata 必须当作 opaque continuation state。

已落地保留：

| Provider format | 保留内容 | 用途 |
| --- | --- | --- |
| Claude Messages | assistant `content` blocks，包括 `thinking`、`tool_use`、signature | 同 provider 续接时原样回放 |
| Gemini | response `content.parts` 与 `thoughtSignature` | 同 provider 续接时原样回放 |
| Gemini Interactions | raw `outputs` | 同 provider 续接时原样回放 |
| OpenAI Responses | raw `output` items 与 `responseId` | function call output / reasoning continuation |

禁止：

- 重写 provider 返回的 tool call id。
- 缺失 id 时自动生成 fallback id。
- 丢弃 Claude / Gemini / Responses 的 native tool metadata。
- 把 Gemini thought signature 或 Claude thinking signature 压成普通文本。
- 把 OpenAI encrypted reasoning 当作可解释内容。

同 provider continuation 所需 native state 丢失时，应 fail-fast 或测试失败。跨 provider switch 只能迁移 canonical 语义，不能伪装 provider-private state 已迁移。

## 5.1 Provider State Contract

`provider_state` 由 Agent runtime 初始化，由 `AgentModelGateway` 在每轮模型调用后更新。

当前契约：

- 初始状态只包含 `sessionId = runId`。
- gateway 会把该状态以内部字段 `_tauritavern_provider_state` 写入 ChatCompletion payload。
- LLM API log 与真正发往 provider 的 payload 都必须剥离 `_tauritavern_provider_state`。
- 每轮成功后，gateway 返回 `sessionId`、`chatCompletionSource`、`providerFormat`、`messageCursor`、`lastResponseId`。
- OpenAI Responses 会额外返回 `transport: "responses_websocket"` 与 `previousResponseId`。
- OpenAI Responses 续接时，gateway 根据 `messageCursor` 只发送新消息，并注入 `previous_response_id`。
- Claude / Gemini / OpenAI Responses / Gemini Interactions 会记录 `nativeContinuation`；tool call 存在但 native metadata 丢失时 fail-fast。

OpenAI Responses Agent 路径使用 persistent WebSocket session。session 由 `sessionId` 复用，建连复用 `HttpClientPool` 的 ChatCompletion WebSocket profile；run 完成、失败或取消后异步关闭，关闭动作不得阻塞 run 最终状态落盘。

## 6. Tool Schema

Tool registry 只产 canonical `AgentToolSpec`。

Gateway/payload adapter 在发送前渲染 provider-facing schema：

```text
AgentToolSpec
  -> provider-specific schema sanitizer
  -> OpenAI-compatible function tool shape
  -> existing provider payload builder maps to native shape
```

当前 sanitizer 策略：

- OpenAI-compatible / OpenAI Responses：保留完整 canonical schema。
- Claude Messages：移除轻量元字段，如 `$schema`、`$id`。
- Gemini / Gemini Interactions：移除当前不兼容的 JSON Schema 关键字，如 `$schema`、`$defs`、`additionalProperties`、组合 schema、`const`、`default`、`title` 等。

Canonical schema 不应为某个 provider 被永久降级。

## 7. Tool Call Contract

Tool call id 必须是不透明字符串：

```text
Provider tool call id
  -> AgentToolCall.id
  -> AgentToolResult.call_id
  -> provider tool result id
```

缺失 id 是 `model.invalid_tool_call`，不得静默生成 `tool_call_{index}`。

Tool result 当前会编码为 JSON 字符串，包含：

```json
{
  "ok": true,
  "content": "...",
  "structured": {},
  "errorCode": null,
  "resourceRefs": []
}
```

前 5 轮 `workspace.write_file` / `workspace.apply_patch` 成功结果会被 runtime hydrate：下一轮模型看到完整文件内容，journal/workspace 真相不变。

## 8. Policy

Gateway 必须遵守：

- iOS policy source allowlist。
- iOS policy endpoint override。
- web search/request image capability。
- settings 中的 provider 配置。
- secret 暴露策略。
- prompt caching opt-in/opt-out。
- model/profile tool support 声明。

Policy denied 必须 fail-fast 并写 journal，不允许静默降级为另一个 provider、另一个模型或空工具集。

## 9. Prompt Cache

Prompt cache 是 provider 能力，不是 Agent 自己随意拼 header。

Agent 可以在 canonical request 中表达 cache 意图，但具体是否启用、如何写 header、是否需要 beta header，必须由现有 provider logic 或正式 adapter 决定。

Custom Claude Messages 的 header 兼容策略尤其不能被 Agent 硬编码覆盖。

## 10. Streaming 边界

未来需要 `ModelDelta`，但当前 Agent tool loop 仍是非 streaming。

两种事件流不能混：

```text
Provider stream
  来自 ChatCompletionService/Repository 的 SSE data 或 normalized chunk。

Agent run event stream
  AgentRunEvent：model_delta、tool_call_requested、checkpoint_created 等语义事件。
```

Agent UI 必须订阅 `api.agent.subscribe(runId, handler)` 的 run event，不直接消费 provider raw stream。

## 11. Error Contract

Gateway 错误建议：

```text
model.provider_denied
model.unsupported_tool_call
model.invalid_tool_call
model.request_build_failed
model.request_failed
model.stream_failed
model.cancelled
model.native_metadata_lost
```

`model.native_metadata_lost` 不应静默降级。

## 12. Tests

当前已覆盖：

- canonical response decode。
- missing tool call id fail-fast。
- Gemini schema sanitizer。
- Agent loop 通过 canonical response 推进。
- 前 5 轮 workspace write result hydration。
- OpenAI Responses native output items 回放。
- OpenAI Responses `provider_state.previousResponseId` 注入与 `messageCursor` 增量输入。
- Claude / Gemini native continuation 计数与缺失 fail-fast。
- same-provider native metadata loss fail-fast。
- cross-provider switch 不迁移 provider-private state。
- LLM API log 剥离 `_tauritavern_provider_state`。
- Claude native content blocks 回放。
- normalizer 保留 Claude/Gemini/OpenAI Responses/Gemini Interactions native metadata。

后续最低补齐：

- prompt cache 与 provider-native state 共存。
- stream `ModelDelta` 不泄漏 raw provider event。
- persistent provider session close 不阻塞 Agent 最终状态。
