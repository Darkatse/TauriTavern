# 请求查看器

本文说明一条 LLM 请求如何被记录，结构化视图与可读文本如何从记录派生，以及发送前的请求预览。接口契约见 [Dev API](../API/Dev.md)。

## 1. 数据从哪里来

一条聊天补全请求经过三层数据，查看器读的是第三层。

| 层 | 位置 | 内容 |
| --- | --- | --- |
| 组装 | 前端 `ChatCompletion` | 条目来源、注入分段、组装时估算的 token、聊天楼层 |
| 生成 payload | 前端交给 Rust 命令 | OpenAI 兼容的中间格式，含扩展字段、工具与生成参数 |
| 线路 payload | 准备完成之后 | 各家原生请求格式 |

线路 payload 是格式构造、Additional Parameters 的 body 覆盖与 Prompt Caching 之后交给 repository 的请求体。日志记录它，预览返回它，结构化视图从它派生。

记录的响应是 repository 交给上层的内容，不是服务商原文：

- 非流式是归一化后的 `chat.completion`，消息的 `native` 保存服务商的原生内容；Cohere 是服务商原文，消息在顶层的 `message`。
- 流式是交给前端的事件流，各格式的事件见[原生模型 API](NativeApiFormats.md#1-格式与链路)。Cohere 是它自己的事件；Bedrock 上的非 Claude 模型由适配器改写成只有 `content_block_delta` 的文本帧。
- 文本补全是服务器的响应或事件，Ollama 的逐行 JSON 已改写为文本补全事件。

## 2. 管线

从请求到可读文本分五段，每段只做一件事。

| 段 | 输入与输出 | 规则 |
| --- | --- | --- |
| 准备 | 生成 payload → 线路 payload | 没有副作用，发送与预览共用 |
| 记录 | 线路 payload 与响应 → 文件 | 一条记录要么文件齐全且可见，要么不存在 |
| 合并 | 事件流 → 响应文档 | 只重组收到的内容，不解释 |
| 投影 | 线路形状 → 结构化视图 | 格式的语义只在这里 |
| 渲染 | 结构化视图 → 可读文本 | 标记与视图的部件一一对应 |

## 3. 准备与预览

- 流式发送、非流式发送与预览调用同一个准备函数，它完成从生成 payload 到线路 payload 的全部步骤。
- 准备只读取。Prompt Caching 用上一轮保存的摘要放置断点，本轮的新摘要随准备结果返回，由发送在调用 repository 之前写回。预览不写回，所以预览不改变下一次发送的断点，预览到的 `cache_control` 与紧随其后的发送相同。
- 预览不登记生成请求，不询问[用户端点](UserEndpointAccess.md)授权，不调用 repository，不联网。
- 预览交给宿主的只有来源、base URL、端点路径与线路 payload，不含连接配置里的密钥与请求头。
- 日志与预览由同一个函数得出脱敏后的请求体、`endpoint` 与 `model`，预览到的请求与随后记录的请求相同。

## 4. 记录

- 每条记录有三个文件：`llm-api-<id>.request.json`，`.response.json` 或 `.response.sse`，`.meta.json`。索引是 `llm-api-index.json`。meta 只有索引字段、错误信息与响应文件的种类，不保存派生内容。
- 发送路径不格式化请求，流式转发只把事件写进文件。视图与可读文本在读取时派生。
- 一条记录在后台任务中按请求文件、响应文件、meta、索引与 `tauritavern-llm-api-log` 事件的顺序完成，不阻塞响应返回。任何一个文件写失败时，记一条诊断日志，删除这条记录已写下的文件，不更新索引，不发事件。
- 日志故障不改变生成结果，也不影响流式事件的转发，不触发 backend error toast。
- 流式生成被调用方中途放弃时，记录照常完成：结果记为取消，响应是已经收到的事件。被取消的非流式请求没有响应，不留记录。
- 由此得到两条不变式：
  - 索引或事件里出现的条目，文件都已可读；
  - `responseRawKind` 给出响应文件的种类。转发事件流的请求一定带 `.response.sse`；一次取回完整响应的请求成功时带 `.response.json`，失败时为空，没有响应体。`stream` 只表示请求是否以流式发出，不决定响应文件的种类。
- `.response.sse` 每行一个事件，由写入端保证。事件内的换行只可能是 JSON 记号之间的空白，写入时换成空格。
- 文本补全的来源写作 `text_completion:<type>`，辅助请求不记录。
- 写入前的脱敏只作用于日志与预览的副本，不修改出站 payload：
  - 移除内部的 `_tauritavern_provider_state`；
  - base64 `data:` URL、同一对象内带非空 `data` 与媒体类型（`media_type`、`mime_type` 或 `mimeType`）的内联媒体、`input_audio`（`format` 与 `data`），替换为只含 MIME 与编码长度的占位标记。

## 5. 结构化视图

视图的字段见 [Dev API](../API/Dev.md#4-requestinspectorproject-contract)，类型定义在 `tt-contracts`。

### 请求

- 来源以 `text_completion:` 开头的是文本补全，视图只有 `prompt` 与参数；其余是聊天补全。
- 聊天请求按请求体自身的容器键识别：`contents`（Gemini generateContent）、`input`（OpenAI Responses、Gemini Interactions）、`messages`（OpenAI 兼容、Claude Messages、Cohere、Bedrock 等）。
- 顶层系统指令（`system`、`systemInstruction`、`system_instruction`、`instructions`）、`tools` 与工具选择各有字段，其余顶层字段按原顺序归入 `parameters`。
- 视图不认识的结构归入 `other`，原样给出。

### 响应

流式事件先合并成与非流式响应同形状的文档，再由同一个入口投影。同一份内容，流式与非流式得到相同的候选。

合并只重组收到的内容：

- chunk 按候选索引归并到 `choices[i].message`；正文与推理按字符串追加，工具调用按工具索引归并，没有索引时按 `id`，其余字段取最新的非空值。
- Anthropic 的内容块按 `index` 保存，增量追加到所属的块；增量没有对应的开始事件时，按增量类型建一个块。结果写在候选零的 `native.claude.content`。
- Gemini 的 parts 追加到候选零的 `native.gemini.content`，相邻的同类文本并成一个 part。
- Cohere 的事件并入候选零的消息，内容块与工具调用取事件的 `index`。
- 文本补全按候选索引归并正文与推理。
- 用量按键合并，流内的错误事件保存在文档顶层。
- 只有收到过的候选、工具调用与内容块才存在，没有补位。

投影规则：

- 候选来自 `choices`。Cohere 的非流式响应来自顶层的 `message`。文本补全的候选来自 `choices`、llama.cpp 的输出数组或单个输出。其他形状整块归入 `other`。
- 消息带 `native` 时，内容取 `native`，否则按 OpenAI 形状的消息投影。两者是同一份内容，只显示一份。
- 可见推理只取一处：非空的 `reasoning_content`，其次 `reasoning`，最后 `reasoning_details` 里的文本与摘要。加密的推理状态显示为 `nativeState`。
- 空字符串不产生文本部件，带缓存断点的除外；其余文本原样保留。
- 候选没有角色时是 `assistant`。
- `finishReason` 取第一个带结束原因的候选，用量取文档顶层。两者读的是记录下来的值：流式响应里是服务商的原文，非流式响应里是适配器归一化后的值。
- `error` 是记录的失败信息；没有时取流内的错误事件。流内错误不会使这条记录标为失败，只能从这里看到。

合并规则与推理的取值顺序在适配器里另有一份，供实时生成使用。适配器遇到中途结束的流、多个候选或错误事件即报错，而这些正是日志要显示的内容，所以查看器保留一份不拒绝输入的实现。修改其中一份时核对另一份。

## 6. 可读文本

`getPreview` 的 `requestReadable` 与 `responseReadable` 由结构化视图渲染，规则见 [Dev API](../API/Dev.md#3-llmapilogs)。渲染器不知道内容来自哪种线路格式，也不知道来自流式还是非流式。

## 修改入口

- 视图类型：`src-tauri/crates/tt-contracts/src/observability.rs`
- 记录与存储：`src-tauri/crates/tauritavern/src/infrastructure/logging/llm_api_logs/` 的 `request.rs`、`recording.rs`、`store.rs`、`files.rs`
- 合并、投影与渲染：同目录 `readable/` 的 `stream.rs`、`projection.rs`、`text_completion.rs`、`render.rs`；脱敏在 `wire.rs`
- 准备与预览：`src-tauri/crates/tt-application/src/services/chat_completion_service/mod.rs`
- 命令与 Hub：`presentation/commands/` 的 `chat_completion_commands.rs`、`dev_logging_commands.rs`，`app/dev_observability.rs`
- 宿主 API：`src/tauri/main/api/dev.js`、`src/types.d.ts`
- 适配器里的合并与归一化：`src-tauri/crates/tt-adapter-provider-http/src/http_chat_completion_repository/` 的 `openai.rs`、`claude.rs`、`gemini.rs`、`normalizers.rs`
