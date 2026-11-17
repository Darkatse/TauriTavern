# 文本补全

## 两族的分工

聊天补全由服务器套用对话模板；文本补全由页面套用 Instruct 与 Context 模板，把完整的提示词作为 `prompt` 发出。同一个本地服务器两条路都能用：要服务器的模板，用聊天补全的 Custom；要自己掌握提示词排布、完整采样器、禁用字符串、grammar 与 logprobs，用文本补全。

主 API 选择器只提供宿主实现了的两族（`openai`、`textgenerationwebui`）。设置里保存的是其他主 API 时，加载设置时切回聊天补全并提示一次。

## 类型

宿主实现由用户填写地址的 9 个类型：`ooba`、`vllm`、`aphrodite`、`tabby`、`koboldcpp`、`llamacpp`、`ollama`、`huggingface`、`generic`。类型集合只在 `TextCompletionType`。下拉里的云服务类型可以选，请求时返回 `Unsupported text completion type: <type>`。

地址去掉末尾的 `/` 与 `/v1` 后作为基址，各类型的路径、认证头与请求体规则在服务的类型表：

- 认证头只在有密钥时添加。密钥取当前选中的一条，请求体带 `secret_id` 时取该条，找不到即报错。
- 转发前去掉页面附加的 `type`、`api_type`、`api_server`、`secret_id`。`generic` 只保留 OpenAI completions 字段，`stop` 截到 4 个；`vllm` 只保留 vLLM 字段。
- Ollama 的请求重建为 `raw: true`、`keep_alive: -1`，采样参数放进 `options`；流是逐行 JSON，适配层改写为 `{ choices: [{ text, thinking }] }` 事件并以 `[DONE]` 结束，流中的 `error` 即失败。
- 参数名、别名与流的解析属于页面（`textgen-settings.js`），宿主不改写。

## 路由

`/api/backends/text-completions/status`、`/props`、`/generate` 的请求与响应形状与 SillyTavern 1.19.0 相同：

- 状态返回 `{ result, data }`，`data` 是 OpenAI 形状的模型列表。`ooba` 能读到模型信息时设响应头 `x-supports-tokenization`；`tabby` 没有加载模型时 `result` 为 `None`。
- props 附加 `chat_template_hash`，页面据此推导模板，并读取 `default_generation_settings.n_ctx`。
- 状态与 props 失败时返回宿主错误文本，不是 JSON：部分类型拿到任何 JSON 都会显示为已连接。

## 生成

生成与聊天补全共用生成运行时：流会话与重放、取消、后台租约、完成通知（`type: 'quiet'` 不通知）与 LLM API 日志（来源写 `text_completion:<type>`）。

出错的呈现与聊天补全一致：

- 非 quiet 的失败写成一条角色回复：非流式为 `{ choices: [{ index: 0, text, finish_reason: 'stop' }] }`，流式为一个 `{ choices: [{ index: 0, text }] }` 事件加 `[DONE]`。
- quiet 的非流式请求返回 `{ error: true, status, response }`；quiet 的流在开始前失败时返回 502 `{ error: { message } }`。
- 流开始之后的失败都写成错误事件。

取消时运行时丢弃生成。非流式请求在命令开始时登记，等待端点确认期间的取消同样生效，请求不再发出。KoboldCpp 在连接断开后仍会生成完这一轮，所以已开始的 KoboldCpp 生成被取消后发送 `POST /api/extra/abort`；未开始或正常结束时不发送。

## 授权与 iOS

每个命令先由服务解析出服务器地址，命令按[用户端点](UserEndpointAccess.md)询问一次来源，再执行请求。iOS 的 `llm.text_completions.enabled` 关闭时，服务以 `PermissionDenied` 拒绝，选择器同时去掉文本补全。

Agent 只使用聊天补全。

## 修改入口

- 类型集合：`tt-domain` 的 `text_completion_type.rs`
- 类型表与请求改写：[`text_completion_service/`](../../src-tauri/crates/tt-application/src/services/text_completion_service)，路径、认证头、字段过滤与流格式在 `backend.rs`，Ollama 的请求与模型列表在 `ollama.rs`
- 传输与流解码：[`http_text_completion_repository/`](../../src-tauri/crates/tt-adapter-provider-http/src/http_text_completion_repository)，Ollama 的逐行 JSON 在 `ollama.rs`；错误映射与 SSE、逐行 JSON 的读取与聊天补全共用 [`provider_response/`](../../src-tauri/crates/tt-adapter-provider-http/src/provider_response)
- 中断收尾：`generation_runtime` 的 `InterruptCleanup`
- 路由：[`text-completion-routes.js`](../../src/tauri/main/routes/text-completion-routes.js)；主 API 的可用集合：[`main-api-selector-option-parking.js`](../../src/tauri/main/adapters/st/main-api-selector-option-parking.js)
