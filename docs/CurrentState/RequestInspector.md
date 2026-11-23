# 请求查看器

请求查看器显示一条 LLM 请求实际发出了什么、收到了什么。本文说明这些内容从哪里来，怎样记录，怎样派生成视图。接口见 [Dev API](../API/Dev.md)。

## 1. 管线

| 段 | 输入与输出 | 位置 | 规则 |
| --- | --- | --- | --- |
| 准备 | 生成 payload → 原生 payload | 应用层 | 只读取，发送与预览共用 |
| 构造 | 原生 payload → 线路请求 | 适配器 | 不联网，发送与描述共用 |
| 记录 | 线路请求与响应 → 文件 | 日志装饰器 | 一条记录要么文件齐全且可见，要么不存在 |
| 合并 | 事件流 → 响应文档 | 读取时 | 只重组收到的内容 |
| 投影 | 请求体与响应文档 → 结构化视图 | 读取时 | 格式的语义只在这里 |
| 渲染 | 结构化视图 → 可读文本 | 读取时 | 不知道线路格式 |

- 生成 payload 是前端交给 Rust 命令的中间格式。原生 payload 是格式构造、Additional Parameters 的 body 覆盖与 Prompt Caching 之后的各家原生格式。
- 线路请求是实际发出的请求行、请求头与请求体。请求体由适配器决定，可以与原生 payload 不同。查看器显示的请求都是线路请求。
- 记录的响应是 repository 交给上层的内容，不是服务商原文：非流式是归一化后的文档，流式是交给前端的事件流，见[原生模型 API](NativeApiFormats.md#1-格式与链路)。

## 2. 准备与预览

- 发送与预览调用同一个准备函数。准备不写任何状态：Prompt Caching 的新摘要随结果返回，由发送写回，预览不写回。预览到的缓存断点因此与紧随其后的发送相同，预览也不改变下一次发送。
- 预览在准备之后向 repository 取线路请求，按生成 payload 的 `stream` 取流式或非流式的那一种。它不发送，也不需要[用户端点](UserEndpointAccess.md)授权。
- 日志与预览由同一个函数得出请求体、`endpoint` 与 `model`，预览到的请求与随后记录的相同。`model` 取自原生 payload，请求体里不一定有它。
- 连接配置不离开应用层。预览失败只回给调用方，不作为后端错误提示。

## 3. 线路请求

- 适配器分两步发送：先不取用 HTTP 客户端，定出方法、URL、请求头与请求体；再取用客户端发送。`describe_request` 只走第一步，它给出的就是会发出的请求。
- 需要联网换取的凭据不在第一步换取，描述时用占位值。
- 描述回答请求怎样构造，不回答它是否发出。调用被拒绝或发送失败时，记录里仍有描述。
- 请求头列出请求自身设置的头与客户端的 `User-Agent`。HTTP 客户端另外补上的头，以及握手时每次连接重新生成的头，不在其中。
- 运行在 WebSocket 会话上的请求，描述的是建立会话的握手，请求体是开始这一轮的消息。
- 构造不出请求时没有描述，记录的请求体是原生 payload。这样的请求也发不出去，原因由发送报出；预览直接失败。

凭据在适配器内替换为 `[redacted]`。请求头或查询参数有值，并满足任一条件，就是凭据：

- 名称含 `auth`、`cookie`、`key`、`password`、`secret` 或 `token`；
- 值等于连接配置的 API key 或鉴权头。

其余的头给出实际发出的值，用户填写的附加头也在其中。

## 4. 记录

- 一条记录是三个文件：请求体，响应（`.response.json` 或 `.response.sse`），meta。meta 保存索引字段、错误信息、响应文件的种类与请求描述，不保存派生内容。
- 记录在后台任务中完成，不阻塞响应。任一文件写失败时删除已写下的文件，不更新索引，不发事件。日志故障不影响生成结果与流式转发。
- 索引或事件里出现的条目，文件都已可读。
- `responseRawKind` 给出响应文件的种类：转发事件流的请求是 `sse`；一次取回完整响应的请求成功时是 `json`，失败时没有响应体。`stream` 只表示请求是否以流式发出。
- 流式生成被调用方中途放弃时，记录照常完成：结果记为取消，响应是已收到的事件。被取消的非流式请求不留记录。
- 写入前把内联媒体换成只含类型与长度的占位标记。这只作用于日志与预览的副本。
- 导出调试包时 meta 按键名再做一次凭据替换。请求头保存在 `headers` 键下，被整体换掉。
- 文本补全的来源写作 `text_completion:<type>`，辅助请求不记录。

## 5. 视图

类型定义在 `tt-contracts`，字段见 [Dev API](../API/Dev.md#4-requestinspectorproject-contract)。

- 请求视图从请求体投影，按请求体自身的容器键识别格式。`http` 是记录下来的请求描述，不从请求体派生。
- 流式事件先合并成与非流式响应同形状的文档，再由同一个入口投影。同一份内容在流式与非流式下得到相同的候选。合并只重组收到的内容，不补位。
- 消息带 `native` 时内容取 `native`，否则按 OpenAI 形状投影。两者是同一份内容，只显示一份。
- 可见推理只取一处，加密的推理状态显示为 `nativeState`。视图不认识的结构归入 `other`，原样给出。
- 结束原因与用量读的是记录下来的值：流式是服务商的原文，非流式是适配器归一化后的值。
- `error` 是记录的失败信息，没有时取流内的错误事件。流内错误不会使记录标为失败，只能从这里看到。
- 可读文本由结构化视图渲染，渲染器不知道内容来自哪种线路格式，也不知道来自流式还是非流式。

合并与推理取值在适配器里另有一份，供实时生成使用。适配器遇到中途结束的流、多个候选或错误事件即报错，而这些正是日志要显示的，所以查看器保留一份不拒绝输入的实现。修改一份时核对另一份。

## 修改入口

Rust 路径相对 `src-tauri/crates/`。

- 视图类型：`tt-contracts/src/observability.rs`
- 准备与预览：`tt-application/src/services/chat_completion_service/mod.rs`
- 构造与凭据替换：`tt-adapter-provider-http/src/provider_request.rs`，各家的 `generation_request` 在 `http_chat_completion_repository/` 与 `http_text_completion_repository/`；端口在 `tt-ports/src/repositories/`
- 记录与存储：`tauritavern/src/infrastructure/logging/llm_api_logs/` 的 `request.rs`、`recording.rs`、`store.rs`、`files.rs`
- 合并、投影与渲染：同目录 `readable/`
- 适配器里的另一份合并：`http_chat_completion_repository/` 的 `openai.rs`、`claude.rs`、`gemini.rs`、`normalizers.rs`
- 调试包：`tauritavern/src/infrastructure/logging/dev_bundle.rs`
- 命令与宿主 API：`tauritavern/src/presentation/commands/`、`tauritavern/src/app/dev_observability.rs`，`src/tauri/main/api/dev.js`、`src/types.d.ts`
