# Agent Provider State Contract

最后更新：2026-05-02

本文档记录当前 **已落地** 的 Agent `provider_state` 契约。它不是阶段计划；后续开发应以这里为当前行为基线。

## 1. 目标

`provider_state` 是一次 Agent run 内的 provider continuation state。

它解决三个问题：

- 让 Agent 在不绕过 `ChatCompletionService` 的前提下复用 provider 原生续接能力。
- 让 OpenAI Responses 使用持久 WebSocket、incremental input 与 `previous_response_id`。
- 让 Claude / Gemini / OpenAI Responses / Gemini Interactions 的 native metadata 丢失时能够 fail-fast，而不是静默退化成普通文本。

`provider_state` 不是 prompt 内容，不是用户设置，也不是要发送给上游 provider 的公开字段。

当前实现位置：`agent_model_gateway/provider_state.rs` 负责注入内部字段与生成下一轮 state；`agent_model_gateway/providers/responses.rs` 负责 OpenAI Responses 的 `previous_response_id` 与增量输入规则。

## 2. 当前字段

Agent run 初始化时，`prepare_agent_tool_request()` 写入：

```json
{
  "sessionId": "<run-id>"
}
```

每轮模型调用成功后，`AgentModelGateway` 返回新的 `provider_state`：

```json
{
  "sessionId": "<run-id>",
  "chatCompletionSource": "openai|custom|claude|makersuite|...",
  "providerFormat": "openai_compatible|openai_responses|claude_messages|gemini|gemini_interactions",
  "messageCursor": 12,
  "lastResponseId": "resp_...",
  "nativeContinuation": {
    "provider": "openai_responses|claude|gemini|gemini_interactions",
    "partCount": 1
  }
}
```

OpenAI Responses 额外写入：

```json
{
  "transport": "responses_websocket",
  "previousResponseId": "resp_..."
}
```

字段语义：

- `sessionId`：run-scoped provider session id；当前等于 `runId`，缺失会 fail-fast。
- `chatCompletionSource`：本轮实际使用的 `ChatCompletionSource::key()`。
- `providerFormat`：gateway 判定后的 provider payload format。
- `messageCursor`：本轮请求成功发送时的 canonical message 数量，用于下一轮 OpenAI Responses incremental input。
- `lastResponseId`：本轮 provider/normalized response id；可能为空，但 OpenAI Responses continuation 必须存在。
- `nativeContinuation`：同 provider 续接需要的 native metadata 计数。native provider 在返回 tool call 但 native part 缺失时 fail-fast。
- `previousResponseId`：OpenAI Responses 下一轮请求的 `previous_response_id` 来源。
- `transport`：当前仅用于标记 OpenAI Responses persistent WebSocket continuation。

## 3. 请求侧流转

`agent_model_gateway::encode` 会把 `provider_state` 复制到 ChatCompletion payload 顶层内部字段：

```json
{
  "_tauritavern_provider_state": { "...": "..." }
}
```

该字段只在 TauriTavern 内部使用：

- payload builder 可以保留它，以便 repository 判断是否需要 persistent provider session。
- LLM API log 会在 raw/readable request 生成前剥离它。
- OpenAI Responses repository 在真正发往上游前剥离它。
- WebSocket `response.create` payload 会剥离 `_tauritavern_provider_state`、`stream` 与 `background`。

不得把 `_tauritavern_provider_state` 当作上游 API 字段、用户设置字段或 SillyTavern 兼容字段。

## 4. OpenAI Responses 增量输入

OpenAI Responses 首轮没有 `previousResponseId` 时，gateway 发送完整 canonical messages。

从第二轮开始：

- 必须存在 `previousResponseId`。
- 必须存在合法 `messageCursor`，且 `messageCursor <= messages.len()`。
- gateway 只发送 `messages[messageCursor..]`。
- 该尾部消息会过滤掉 assistant messages，只保留新产生的 tool/user/developer 等需要追加给 provider 的输入。
- gateway 同时在 payload 顶层写入 `previous_response_id`。

这使 Agent 可以依赖 Responses 侧的 previous response state，而不必每轮重放全部 assistant native output。

## 5. OpenAI Responses 持久 WebSocket

当 OpenAI Responses payload 带有 `_tauritavern_provider_state.sessionId` 时，repository 使用 run-scoped persistent WebSocket session：

```text
sessionId -> ResponsesWsSessionPool -> response.create -> response.completed
```

当前行为：

- 同一个 `sessionId` 会复用同一条 WebSocket，除非 base URL / endpoint 的 connection key 变化。
- WebSocket 建连复用 `HttpClientPool` 的 ChatCompletion WebSocket profile，通过统一 HTTP client 发起 Upgrade；代理、TLS/client 构建与连接超时语义与 ChatCompletion transport 对齐。
- connection key 包含 transport revision；request proxy / client 配置变更会触发 session 重建。
- persistent session 路径失败时直接返回错误，不做 HTTP fallback。
- session 出错时会从 pool 移除。
- Agent run 完成、失败或取消后，runtime 会在最终状态写入之后异步关闭 provider session，清理动作不阻塞 `awaiting_commit` / `failed` / `cancelled` 落盘。

## 6. Native Metadata Fail-Fast

这些 provider format 被视为 native continuation provider：

| providerFormat | nativeContinuation.provider |
| --- | --- |
| `openai_responses` | `openai_responses` |
| `claude_messages` | `claude` |
| `gemini` | `gemini` |
| `gemini_interactions` | `gemini_interactions` |

如果 response 中存在 tool calls，但对应 native part 计数为 0，gateway 返回：

```text
model.native_metadata_lost
```

这是刻意的 fail-fast 契约。不得用普通文本、合成 id 或空 native state 静默替代 provider-private continuation state。

## 7. 可观测性

Agent run event 当前会记录：

- `provider_state_updated`：只记录摘要字段，便于诊断 continuation。
- `model_response_stored`：保存完整 `AgentModelResponse` 到 `model-responses/round-XXX.json`。
- `model_completed`：包含 `modelResponsePath`，指向本轮模型响应文件。

`model-responses/` 是 run workspace 内部诊断目录，不属于模型工具可见 roots。

LLM API log 当前记录剥离内部 provider state 后的 request raw/readable。日志表示“实际 provider payload 视角”，不暴露 `_tauritavern_provider_state`。

## 8. 维护约束

- Agent LLM 调用必须继续走 `ChatCompletionService::generate_exchange_with_cancel()`，不能为了 continuation 绕过 ChatCompletion service/payload/logging/policy 链路。
- `provider_state` 缺失必要字段时必须 fail-fast。
- `messageCursor` 是 canonical messages 的游标，不是 provider payload item 数。
- `previousResponseId` 与 `lastResponseId` 是 provider opaque id，不得解析其格式。
- `_tauritavern_provider_state` 只能在内部层间传递，最终日志与上游 payload 都必须剥离。
- 非 Agent 的 Legacy Generate / SillyTavern 事件语义不得依赖该字段。
