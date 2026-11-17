# 文本补全

聊天补全由服务器套用对话模板；文本补全由页面套用 Instruct 与 Context 模板，把完整的提示词作为 `prompt` 发出。同一个本地服务器两条路都能用：要服务器的模板，用聊天补全的 Custom；要自己掌握提示词排布、采样器、grammar 与 logprobs，用文本补全。

## 可用范围

- 主 API 的可用集合由 parking 适配器决定：宿主实现的聊天补全、文本补全与 [AI Horde](Horde.md)（`openai`、`textgenerationwebui`、`koboldhorde`），iOS 关闭文本补全或 Horde 时去掉对应一项。选择器、`/api` 命令与 Connection Manager 都按它判断；设置里保存的主 API 不在集合内时，加载设置时切回聊天补全并提示一次。
- Connection Manager 的配置可以是文本补全；Agent 只使用聊天补全。
- 宿主实现由用户填写地址的类型，类型集合在 `TextCompletionType`。下拉里的云服务类型可以选，请求时报告不支持。

## 宿主的职责

宿主按上游契约做一层薄代理：解析地址（末尾的 `/v1` 可有可无），按类型选路径、加认证头、过滤请求体，再转发。这些规则都在服务的类型表里。参数名、别名与流的解析属于页面，宿主不改写；Ollama 的接口与其他类型不同，所以它的请求由服务重建，逐行 JSON 由适配层改写为文本补全事件。

密钥选取与聊天补全共用一条规则：请求带 `secret_id` 时取该条，找不到即报错；否则取当前选中的一条。

## 路由

路由的请求与响应形状与 SillyTavern 1.19.0 相同。宿主另外遵守以下约定：

- 状态与 props 失败时返回宿主错误文本，不返回 JSON，因为页面拿到 JSON 就可能显示为已连接。
- 远程分词由服务器自己的分词器计数，响应带 `fallback: false`，页面因此缓存这个计数；失败时返回 `{ error: true }`，页面改用本地计数。
- 模型下载在服务器下载完成后才返回。Tabby 的密钥不是 admin 时返回 403，页面据此提示。
- KoboldAI Classic 的分词路由返回 501。

## 生成

生成与聊天补全共用生成运行时（流会话、取消、后台租约、完成通知）与 [LLM API 日志](LoggingObservability.md)。

出错的呈现见[前端宿主契约](../FrontendHostContract.md#43-路由表public)的生成路由；quiet 的非流式请求失败时返回上游的 `{ error: true, status, response }`。

取消时运行时丢弃生成。KoboldCpp 在连接断开后仍会生成完这一轮，所以已开始的生成被取消时另发 `POST /api/extra/abort`。

消息头的缓存命中率与聊天补全在同一处读取，后端不报告提示词缓存时不显示。

## 授权与 iOS

每个命令先解析服务器地址，再按[用户端点](UserEndpointAccess.md)询问一次来源，然后发出请求。[iOS 策略](iOSPolicy.md)在解析地址时检查。

## 修改入口

- 类型集合：`tt-domain` 的 `text_completion_type.rs`
- 类型表与请求构造：[`text_completion_service/`](../../src-tauri/crates/tt-application/src/services/text_completion_service)，类型表在 `backend.rs`，Ollama 的请求与模型列表在 `ollama.rs`
- 传输与流解码：[`http_text_completion_repository/`](../../src-tauri/crates/tt-adapter-provider-http/src/http_text_completion_repository)；与聊天补全共用的错误映射与流读取在 [`provider_response/`](../../src-tauri/crates/tt-adapter-provider-http/src/provider_response)
- 中断收尾：`generation_runtime` 的 `InterruptCleanup`
- 路由：[`text-completion-routes.js`](../../src/tauri/main/routes/text-completion-routes.js)；远程分词在 [`tokenizer-routes.js`](../../src/tauri/main/routes/tokenizer-routes.js)
- 主 API 的可用集合：[`main-api-selector-option-parking.js`](../../src/tauri/main/adapters/st/main-api-selector-option-parking.js)
- 缓存命中率的读取：[`prompt-cache-usage.js`](../../src/scripts/util/prompt-cache-usage.js)
