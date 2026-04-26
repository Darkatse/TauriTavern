# TauriTavern Agent LLM Gateway

本文档定义 Agent Runtime 与现有 LLM 调用链之间的边界。

结论先行：Agent 需要 provider-agnostic `LlmGateway`，但第一阶段必须复用现有 `ChatCompletionService`，不能新建一套绕过现有 policy、日志、proxy、secret、prompt cache 的 HTTP 路径。

## 1. Ground of Truth

当前后端 LLM 事实：

- Provider source 定义在 `ChatCompletionSource`，代码已覆盖 OpenAI、OpenRouter、Custom、Claude、Makersuite/Gemini、VertexAI、DeepSeek、Cohere、Groq、Moonshot、NanoGPT、Chutes、SiliconFlow、Zai 等。见 `src-tauri/src/domain/repositories/chat_completion_repository.rs:8`。
- Payload builder 由 `application/services/chat_completion_service/payload/mod.rs` 按 provider 分发。见 `src-tauri/src/application/services/chat_completion_service/payload/mod.rs:25`。
- `ChatCompletionService` 负责 source 解析、iOS policy、endpoint override、feature policy、settings、secret、prompt caching、payload build、generate/generate_stream/cancel。见 `src-tauri/src/application/services/chat_completion_service/mod.rs:302`、`src-tauri/src/application/services/chat_completion_service/mod.rs:358`。
- LLM API 日志依赖 `LoggingChatCompletionRepository` wrapper。见 `src-tauri/src/app/bootstrap.rs:372`。
- Custom Native API 文档强调 tool_call_id 透明性与 native metadata 保真。见 `docs/CurrentState/NativeApiFormats.md:145`。

因此 Agent 文档不要复制旧 provider 列表，也不要以 `docs/BackendStructure.md` 中较旧的 provider 描述作为完整事实源。以代码与 `docs/CurrentState/NativeApiFormats.md` 为准。

## 2. Gateway 目标

`LlmGatewayService` 的目标：

- 给 Agent Runtime 提供统一的 `ModelRequest` / `ModelResponse` / `ModelDelta`。
- 隔离 OpenAI、Claude、Gemini、Responses、OpenAI-compatible、Custom Native 等格式差异。
- 保留 native metadata、tool_call_id、reasoning、prompt cache、provider-specific flags。
- 复用现有 `ChatCompletionService` 的 policy/logging/cancel 能力。
- 让 Profile 切换模型时，Agent loop 不关心 provider payload 细节。

它不是：

- 新 HTTP client。
- 新 provider registry。
- 绕过 `ChatCompletionService` 的捷径。
- MCP Sampling 的实现。

## 3. Phase 1 Wrapper

当前状态（2026-04-26）：Phase 2A 已有轻量 `AgentModelGateway` wrapper，复用 `ChatCompletionService::generate_with_cancel()` 驱动非 streaming 工具循环。完整 provider-agnostic `ModelRequest` / `ModelResponse` 抽象仍未落地。

Phase 1 建议：

```text
AgentRuntimeService
  -> LlmGatewayService
    -> ChatCompletionService.generate / generate_stream
      -> ChatCompletionRepository
        -> LoggingChatCompletionRepository
          -> HttpChatCompletionRepository
```

输入仍可由 `PromptSnapshot` 提供，并转为现有 `ChatCompletionGenerateRequestDto`。

要求：

- 使用现有 source/model/settings/payload 字段。
- 使用现有 cancellation registry 或等价 watch channel。
- 使用现有 logging wrapper。
- 使用现有 prompt cache policy。
- 保留 provider-native metadata。
- 失败时写 Agent journal。

禁止：

- 直接调用 `HttpChatCompletionRepository`。
- 绕过 `HttpClientPool`。
- 自行读取 secret。
- 自行拼 endpoint override。
- 因 Agent Mode 而关闭 LLM API log。

## 4. 长期 ModelRequest

长期建议领域模型：

```rust
ModelRequest {
    request_id,
    profile_id,
    model,
    messages,
    tools,
    tool_choice,
    response_format,
    generation_config,
    cache_policy,
    metadata,
}
```

`ModelMessage`：

```rust
ModelMessage {
    role,
    name,
    content,
    native,
}
```

`ContentPart`：

```text
Text
Image
File
Resource
ToolCall
ToolResult
Reasoning
Native
```

`native` 必须能保留 provider-specific block，不得清洗未知字段。

## 5. Tool Call 透明性

Tool call id 必须当作不透明字符串。

禁止：

- 假设 tool_call_id 是 OpenAI UUID。
- 重写 provider 返回的 call id。
- 丢弃 Claude/Gemini/Responses 的 native tool metadata。
- 把 Gemini thought signature/native blocks 压平成普通文本。

原因：某些 provider 的后续 stateless history 或 tool loop 需要原样回放 native metadata。

## 6. Streaming 边界

Agent 有两种事件流：

```text
Provider stream
  来自 ChatCompletionService/Repository 的 SSE data 或 normalized chunk。

Agent run event stream
  AgentRunEvent：model_delta、tool_call_requested、checkpoint_created 等语义事件。
```

两者不能混用。

当前 `ChatCompletionStreamEvent::Chunk { data }` 只是 provider SSE data 桥接，不是 Agent timeline event。Agent UI 必须订阅 `api.agent.subscribe(runId, handler)` 的 run event。

## 7. Cancellation

Agent cancel 应传播到当前模型请求。

要求：

- 每个 model request 有 request id。
- request id 遵守现有后端 cancel id 风格：短、稳定、可校验。
- run cancel 写 `run_cancel_requested`，再 signal model request。
- model cancelled 不是 failure，run 进入 `Cancelled` 或按 state machine 停止。

如果模型请求已经完成但 commit 未开始，cancel 必须阻止 commit。

## 8. Policy

Gateway 必须遵守：

- iOS policy source allowlist。
- iOS policy endpoint override。
- web search/request image capability。
- settings 中的 provider 配置。
- secret 暴露策略。
- prompt caching opt-in/opt-out。
- model/profile tool support 声明。

Policy denied 必须 fail-fast 并写 journal，不允许静默降级为另一个 provider 或另一个模型。

## 9. Prompt Cache

Prompt cache 是 provider 能力，不是 Agent 自己随意拼 header。

Agent 可以在 `ModelRequest.cache_policy` 中表达意图，但具体是否启用、如何写 header、是否需要 beta header，必须由 existing provider logic 或正式 adapter 决定。

Custom Claude Messages 的 header 兼容策略尤其不能被 Agent 硬编码覆盖。

## 10. Error Contract

Gateway 错误建议：

```text
model.provider_denied
model.unsupported_tool_call
model.request_build_failed
model.request_failed
model.stream_failed
model.cancelled
model.native_metadata_lost
```

`model.native_metadata_lost` 应作为开发期 fail-fast 或测试失败，不应在生产静默发生。

## 11. Tests

最低测试：

- Agent gateway 通过 mock `ChatCompletionService` 验证不直接打 HTTP。
- provider source denied 时 run failed。
- stream chunk 转为 `model_delta`，但不泄漏为 raw provider event。
- tool_call_id opaque round-trip。
- Gemini native metadata round-trip。
- cancel propagates。
- LLM API log 不因 Agent 关闭。
