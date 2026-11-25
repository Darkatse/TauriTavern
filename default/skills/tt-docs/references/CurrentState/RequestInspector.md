# 请求查看器

请求查看器显示一条 LLM 请求实际发出了什么、收到了什么，以及下一次发送会发出什么。本文说明这些内容从哪里来。接口见 [Dev API](../API/Dev.md)。

## 1. 管线

| 段 | 输入与输出 | 位置 | 规则 |
| --- | --- | --- | --- |
| 组装 | 聊天与预设 → 生成 payload | 前端 | dry run 与发送共用 |
| 准备 | 生成 payload → 原生 payload | 应用层 | 不写状态，发送与预览共用 |
| 构造 | 原生 payload → 线路请求 | 适配器 | 不联网，发送与描述共用 |
| 记录 | 线路请求与响应 → 文件 | 日志装饰器 | 一条记录要么文件齐全且可见，要么不存在 |
| 合并 | 事件流 → 响应文档 | 读取时 | 只重组收到的内容 |
| 投影 | 请求体与响应文档 → 结构化视图 | 读取时 | 格式的语义只在这里 |
| 渲染 | 结构化视图 → 可读文本 | 读取时 | 不知道线路格式 |

预览走前三段，日志从第四段开始，两者显示的都是线路请求。它们由同一个函数得出请求体、`endpoint` 与 `model`，预览到的请求就是随后发出并记录的请求，例外见[第 3 节](#3-准备与预览)。

- 生成 payload 是前端交给 Rust 命令的中间格式。
- 原生 payload 是各家的原生格式，已经过 Additional Parameters 的覆盖与 Prompt Caching。
- 线路请求是实际发出的请求行、请求头与请求体。请求体由适配器决定，可以与原生 payload 不同。
- 记录的响应是 repository 交给上层的内容：非流式是归一化后的文档，流式是交给前端的事件流，见[原生模型 API](NativeApiFormats.md#1-格式与链路)。

## 2. 组装

- dry run 与发送走同一段组装代码，消息、工具与生成参数的规则不因 dry run 而不同。Prompt Manager 的刷新、Agent 的输入采集与请求预览都是 dry run。
- dry run 不写聊天记录，不推进世界书的定时效果，不运行生成拦截器。宏照常求值，写变量的宏照常写入。
- 组装带着错误继续时，结果给出原因，已组装的部分照常发出。
- 合并 system 另成一份结果，不改动组装出的消息。dry run 之后 Prompt Manager 持有各条提示词，发送之后持有发出的消息；读 `promptManager.messages` 的扩展依赖后一种状态。

组装出提示词的 dry run 返回 `{ generateData, promptAssembly }`，`promptAssembly` 说明每条消息与每个工具的出处：

- 出处在创建消息的地方写下，在合并 system 之前读取一次。
- 消息由片段组成，片段的出处是 Prompt Manager 提示词、扩展提示词或聊天记录。多个片段只出现在同深度同角色的聊天内注入合成的消息里。
- 附着到已有消息的条目不另成片段，列在那条聊天记录的片段上。
- 片段的正文是拼接时的文本。之后的宏替换改动了它时，在消息里定位不到这一段。

## 3. 准备与预览

- 预览只支持聊天补全下打开的角色聊天。它做一次 `normal` 类型的 dry run，再用发送所用的函数得出生成 payload 与命令参数，交给预览命令。这两个函数不询问用户，也不改连接状态。
- 预览等到没有生成在进行时才开始，这之前到达的预览请求共用这一次。
- Prompt Caching 的新摘要随准备的结果返回，由发送写回。
- 预览命令在准备之后向 repository 取线路请求。它不发送，也不需要[用户端点](UserEndpointAccess.md)授权。
- 预览失败只回给调用方，不作为后端错误提示。

预览与随后的发送在这些地方可以不同：

- 只有发送才做的事不在预览中：`CHAT_COMPLETION_SETTINGS_READY` 监听者的改写、生成拦截器的运行，以及把输入框的内容与开场白的宏展开写进聊天记录。读聊天记录的宏在两边取到的文本因此可以不同。
- 预览的是 `normal` 类型的 Legacy 请求。Agent 模式开启时，它不是 Agent 的首轮请求，也不带 Agent 的会话状态。
- 世界书概率与随机宏每次组装重新取值。

## 4. 线路请求

- 适配器分两步发送：先定出方法、URL、请求头与请求体，再取用 HTTP 客户端发出。`describe_request` 只走第一步，给出的就是会发出的请求。
- 描述回答请求怎样构造，不回答它是否发出。构造不出请求时没有描述，记录的请求体是原生 payload。
- 需要联网换取的凭据不在第一步换取，描述里是占位值。
- 请求头只列请求自身设置的头与客户端的 `User-Agent`。HTTP 客户端另外补上的头、每次连接重新生成的头不在其中。
- 运行在 WebSocket 会话上的请求，描述的是建立会话的握手，请求体是开始这一轮的消息。
- 凭据在适配器内替换为 `[redacted]`。请求头或查询参数的名称表明它是凭据，或它的值等于连接配置里的密钥，就按凭据处理；其余的头是实际发出的值。

## 5. 记录

- 一条记录是请求体、响应与 meta 三个文件。meta 保存索引字段、错误信息、响应文件的种类与请求描述，不保存派生内容。
- 记录在后台任务中完成，不阻塞响应。日志故障不影响生成结果与流式转发。
- 流式生成被调用方中途放弃时，记录照常完成：结果记为取消，响应是已收到的事件。被取消的非流式请求不留记录。
- 写入前把内联媒体换成只含类型与长度的占位标记，预览同样处理。
- 文本补全的来源写作 `text_completion:<type>`。辅助请求不记录。

## 6. 视图

类型定义在 `tt-contracts`，字段见 [Dev API](../API/Dev.md#4-requestinspectorproject-contract)。

- 请求视图从请求体投影，按请求体自身的容器键识别格式。`http` 是记录下来的请求描述，不从请求体派生。
- 流式事件先合并成与非流式响应同形状的文档，再由同一个入口投影。
- 消息带 `native` 时内容取 `native`，否则按 OpenAI 形状投影。两者是同一份内容，只显示一份。
- 可见推理只取一处，加密的推理状态显示为 `nativeState`。视图不认识的结构归入 `other`，原样给出。
- 结束原因与用量读记录下来的值：流式是服务商的原文，非流式是适配器归一化后的值。
- `error` 是记录的失败信息，没有时取流内的错误事件。流内错误不会使记录标为失败。

合并与推理取值在适配器里另有一份，供实时生成使用。适配器遇到中途结束的流、多个候选或错误事件即报错，查看器要显示这些，所以保留一份不拒绝输入的实现。修改一份时核对另一份。

## 7. 界面

界面岛只渲染视图与组装结果，不认识线路格式。入口有两类：Prompt Manager 头部的"预览请求"打开预览，设置、快捷入口与 `/llmlog` 打开最新一条日志。

- 三种看法出自同一份视图。结构是一份大纲，一段一行，点开才是正文；Payload 是请求行、请求头与请求体的树；对比见[第 8 节](#8-对比)。
- 大纲的一行是一个文本段、一次工具调用、一个工具结果、一段思考、一个媒体或一个未识别的块，同一条消息的行共用一条角色色带。连续的对话消息超过 6 条时，除最后 2 条外折成一行。
- 预览的行带出处与 token。出处按发送顺序在线路请求的文本里定位，游标只前进；工具回合按调用 id 对应。定位不到的出处单独列出，不猜位置。
- 树上折叠节点的摘要取自大纲里同一路径的行；没有对应行的对象按通用规则，数组里的对象只列出与兄弟不同的标量。
- 搜索是筛选：结构只留下命中的行，Payload 只留下通向命中的分支。
- 页面上的内容不随请求变大：没有展开的部分不渲染，长正文先截断，子节点分批显示。
- 有含义的颜色是四种角色色与对比的增、删两色，都是 `.tt-ri-root` 上的自定义属性，主题可以覆盖。增删同时有行首的符号。

## 8. 对比

对比把选中的请求与一条基准相比。结果仍是一份大纲，用同一套渲染与搜索：有变化的行逐条列出，相同的行折叠。只比较请求。

- 比较的是每一行发出的内容，文本末尾的空白不算。
- 聊天与预设放置的内容分开对齐，按深度插入的条目才不会随聊天增长显示为变化。
- 两条请求按同一粒度比较。合并消息只在两边都有组装结果时逐段比较，否则两边都按整条消息。
- 基准默认是时间上紧挨在前的一条。预览另有"上一次预览"：与当前预览不同的最近一次预览，由外壳保存，弹窗关闭后仍在。

## 修改入口

Rust 路径相对 `src-tauri/crates/`，前端路径相对 `src/`。

- 组装与来源：`script.js` 的 `GenerateInternal`；`scripts/openai.js` 的 `prepareOpenAIMessages`、`getPromptSources`、`populationInjectionPrompts`、`ChatCompletion.squashSystemMessages`；`scripts/prompt-injections.js`
- 预览编排：`scripts/tauritavern/request-preview.js`；生成 payload 在 `scripts/openai.js` 的 `createCurrentGenerationParameters`，命令参数在 `tauri/main/services/ai/chat-completion-dto.js`
- 视图类型：`tt-contracts/src/observability.rs`
- 准备与预览命令：`tt-application/src/services/chat_completion_service/mod.rs`
- 构造与凭据替换：`tt-adapter-provider-http/src/provider_request.rs`，各家的 `generation_request` 在 `http_chat_completion_repository/` 与 `http_text_completion_repository/`；端口在 `tt-ports/src/repositories/`
- 记录与存储：`tauritavern/src/infrastructure/logging/llm_api_logs/` 的 `request.rs`、`recording.rs`、`store.rs`、`files.rs`
- 合并、投影与渲染：同目录 `readable/`
- 适配器里的另一份合并：`http_chat_completion_repository/` 的 `openai.rs`、`claude.rs`、`gemini.rs`、`normalizers.rs`
- 调试包：`tauritavern/src/infrastructure/logging/dev_bundle.rs`
- 命令与宿主 API：`tauritavern/src/presentation/commands/`、`tauritavern/src/app/dev_observability.rs`，`tauri/main/api/dev.js`、`types.d.ts`
- 界面：`scripts/tauri/setting/request-inspector-app/`（出处定位 `prompt-sources.ts`，大纲 `outline.ts` 与 `outline-message.ts`，树 `json-tree.ts`，对比 `compare*.ts`，序列比较 `text-diff.ts`），外壳与入口 `scripts/tauri/setting/request-inspector.js`
