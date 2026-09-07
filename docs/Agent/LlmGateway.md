# LLM gateway

Agent Loop 使用统一的模型消息、工具调用和工具结果。Gateway 在这套表示与各家模型协议之间转换，并复用项目已有的 ChatCompletionService。

```text
AgentModelRequest
  → AgentModelGateway
  → ChatCompletionService
  → provider repository
  → AgentModelResponse + provider_state
```

连接设置、凭据、请求日志与平台策略继续由原有服务处理。Provider 格式细节集中在 gateway 与对应 adapter，循环只处理模型回合和工具操作。

## 通用内容与原生内容

`AgentModelRequest` 保存消息、工具和续接状态；`AgentModelResponse` 保存 assistant 消息、文本、工具调用与 usage 等信息。Gateway 根据本次工具快照，把模型返回的调用名称还原为稳定工具 ID。

Claude、Gemini、OpenAI Responses 等协议还需要回传原生内容，如 reasoning signature、thought signature 或原生 output blocks。这些内容随消息保留，由对应 provider adapter 原样续接。Timeline 使用 `readModelTurn()` 提供的可见文本和 reasoning 投影。

## 续接状态

`provider_state` 属于一个 Invocation 的模型会话。每轮调用成功后，gateway 更新会话 ID、消息游标和 provider 需要的响应信息，下一轮继续使用。不同 Invocation 各自持有状态。

OpenAI Responses 有两种续接方式：

- 默认通过 HTTP 重放完整消息与原生输出。
- 显式启用 Responses WebSocket 时，复用会话，以 `previous_response_id` 发送新增输入。

内部字段 `_tauritavern_provider_state` 只用于服务间传递，在请求日志和上游 payload 输出前移除。Profile 与提示词无需管理该字段。传输细节见 [原生 API 格式](../CurrentState/NativeApiFormats.md)。

## 流式调用

Profile 的 `run.stream` 控制当前 Invocation 的默认行为，启动参数 `options.stream` 可覆盖整次 Run。当前流式工具参数投影支持 OpenAI Chat-compatible 和 Custom OpenAI Responses 路径。

流式过程供前端显示正在写入或修改的内容，最终完整响应仍进入相同的工具执行路径。不支持的流式路由会返回错误。

## 修改入口

- [agent_model_gateway](../../src-tauri/crates/tt-application/src/services/agent_model_gateway)：`encode`、`decode`、工具 schema 与 provider adapter。
- [provider_state.rs](../../src-tauri/crates/tt-application/src/services/agent_model_gateway/provider_state.rs)：续接状态。
- [ChatCompletionService](../../src-tauri/crates/tt-application/src/services/chat_completion_service/mod.rs)：统一模型调用。
- [tt-adapter-provider-http](../../src-tauri/crates/tt-adapter-provider-http/src)：实际网络请求与 provider 协议。

修改 provider 时，优先验证多轮工具调用中的消息和原生续接内容。代表性覆盖位于 gateway tests 和 host 的 chat-completion contract tests。
