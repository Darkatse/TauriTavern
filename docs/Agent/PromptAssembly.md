# Prompt assembly

Agent 继续使用 SillyTavern 的 PromptManager 组装提示词。Rust 解析 Profile、预设和模型连接，前端用这些设置生成消息，随后由 Rust 运行模型与工具循环。

## 冻结一次输入

Chat 的 `Generate()` 在组装前采集材料，在 `GENERATE_AFTER_DATA` 的异步处理完成后、dry-run 返回前封存 `FrozenRunInputSnapshot`，供 Chat 与 SDK 使用。它记录生成那一刻的聊天事实，不按任何 Profile 裁剪：

| 内容 | 来源和用途 |
| --- | --- |
| `promptInputs` | 聊天、角色、示例、扩展提示词等 PromptManager 输入；世界书按来源保存 |
| `worldInfoActivation` | 本次生成激活的世界书 |
| `macroContext` | 名称、角色模板、聊天事实、设置与模块数据，以及供原生端使用的只读文本 |
| `variables` | local/global 变量原值 |
| `currentPreset` | 当前聊天预设的名称与内容（预设文件格式，含未保存的改动）；未选中预设时名称为 null |
| `currentModelConnection` | 本次生成使用的模型连接 |

根 Agent、子 Agent 和接手的 Agent 共用这份输入，各自按 Profile 决定预设、模型和上下文范围。每次组装使用工作副本，提示词和请求参数通过同一[宏求值入口](../API/Macros.md)共享本次变量，保存的输入保持不变。

Session 从自身历史与本次用户消息准备输入，使用指定预设，不取当前角色聊天的数据。变量来源见 [Session API](../API/Agent.md#持续-session)。完整历史保存在 JSONL 中；宏环境由完整会话求出并在发送时冻结，Run 保留最终消息和组装请求。

捕获保存原始角色模板，不执行其中的宏；原生只读文本复用首次生成的结果。旧 Run 可查看和尝试 resume/fix；需要重新组装时，schemaVersion 1 的输入返回 `agent.frozen_run_input_outdated`，缺少宏重建材料时返回 `agent.macro_context_rebuild_unavailable`。

## 上下文策略

Profile 的 `includeActivatedWorldInfo` 与 Chat 的历史窗口只在 [agent-context-policy.js](../../src/scripts/tauritavern/agent/agent-context-policy.js) 的投影中应用：它从冻结输入生成每次组装的工作输入。世界书只激活一次，前后文、示例、深度注入、outlet 与 Author's Note 附加内容按来源保存，由投影取舍。`includeActivatedWorldInfo` 只约束初始提示词，工具读取完整的激活结果。

live 根生成同样先投影。live `{{outlet}}` 读取全局扩展提示词，因此投影后的世界书注入会写回全局。

## 一种组装，一个 live 例外

每个 Invocation 按自己的 Profile 选择预设与连接：`currentPromptSnapshot` 取冻结的 `currentPreset` 与 `currentModelConnection`，`ref` 与 `connectionRef` 解析具名资源。“当前”是 Run 捕获时的聊天，不随根 Agent 的绑定或运行中的 UI 切换改变。

聊天根 Invocation 绑定当前预设时，直接复用 `Generate()` 的 live 生成，保留扩展钩子对消息和参数的修改。其余情况都独立组装。根绑定指定预设时：

```text
冻结聊天输入
  → promptAssembly.prepare：Rust 解析 Profile、读取预设、应用连接
  → promptAssembly.buildSnapshot：前端 PromptManager 组装消息
  → startRunWithPromptSnapshot：Rust 创建 Run 并开始循环
```

运行中的组装通过同一宿主往返：runtime 写入 `prompt_assembly_requested` 事件，host bridge 按请求 ID 读取组装输入，调用同一个前端组装器，再把结果交回等待中的 Invocation。委派与交接先按接收方 Profile 生成请求；Session 保留发送时的组装请求和冻结输入。页面启动根据 bootstrap 中的活动 Session Run ID，在 `APP_READY` 后重新挂桥，已完成的组装请求在事件重放中跳过。缺少当前预设时返回 `prompt_assembly.current_preset_unavailable`；需要预设身份而当前预设未命名时返回 `agent.current_preset_unnamed`。

`agentSystemPrompt` 是 Profile 指令在 PromptManager 中的位置，`agentTask` 是委派或交接任务的位置。Preset 控制组件的位置和 role，runtime 消费最终组装好的消息。

Snapshot 使用 `messages: AgentModelMessage[]` 与独立的 `generationParameters`，旧 Chat payload 在入口适配。三个快照入口统一通过 `agentPromptModelFacts(settings)` 从有效设置提供 `tokenBudget: { context, response, tokenizerModel }`，Session runtime 据此计数并决定压缩点。

普通 Chat 在 `CHAT_COMPLETION_SETTINGS_READY` 完成后转换最终请求；其中的 live 变量修改不写回冻结输入。SDK 独立组装不触发此事件。

Session 历史由 Rust 选取、校验并组成完整的工具调用与结果组，以 `promptInputs.sessionHistory` 交给 PromptManager。PromptManager 负责布局、历史位置、深度注入和附加提示词，保留原始内容及 metadata，不重新展开历史宏，也不按预算截断历史。未闭合工具组由 Rust 转为中断说明，原历史不改写，工具不重放。Session 不接受历史窗口，`initialChatHistoryMessages` 必须为 `-1`。

普通 Session 轮次沿用已组装请求并追加消息；压缩时才以摘要和保留组替换组装输入，复用本 Run 的组装请求、宏环境及已解析的连接和生成参数。深度注入和附加提示词继续作用于模型当前看到的上下文；附加提示词的正索引从该上下文开头计数，摘要可能成为第一条 user 消息。完整会话的宏值不随压缩倒退。压缩规则见[运行循环](Runtime.md#session-上下文压缩)。

## Skill 与 Agent 目录

Runtime 准备 Invocation 时，将 Skill 与可调用 Agent 目录追加到 `agentSystemPrompt` 正文末尾。默认和自定义指令使用同一流程，保留预设的位置与 role；后续轮次和恢复沿用已准备请求。

Skill 目录与文件视图使用同一份有效绑定；Agent 目录由实际工具与目标调用资格决定。规则分别见 [Skill](Skill.md) 和 [多 Agent 协作](SubAgent.md)，手工 snapshot 的组件标记见 [API](../API/Agent.md)。

## 预设与连接各管什么

Preset 提供提示词布局和生成设置。Profile 的 model binding 提供最终 source、model、endpoint、secret 和路由；这些连接字段会覆盖预设中的旧值。预设字段按 live 加载预设的同一映射转为 settings，映射外的字段原样保留。

Additional Parameters 在前端准备请求时展开，后续轮次复用结果；适用范围见[宏与请求模板](../API/Macros.md#agent-与请求模板)。

显式的 `preset.reasoningEffort` 优先于预设的参数省略设置；实际值由 PromptManager 按 source 和模型归一化，再由 provider adapter 映射协议。组装不修改保存的预设。

`currentPromptSnapshot` 使用输入中冻结的连接。`connectionRef` 则通过 LLM Connection 解析：组装时应用一次，让 PromptManager 使用正确的模型设置；Invocation 进入循环前再应用到最终请求。Model Target 与反向代理预设的管理见 [LLM Connection API](../API/LlmConnections.md)。

独立组装使用 headless PromptManager，不切换 UI 的预设或模型。动态扩展提示词在 live 组装前采集；依赖普通 `CHAT_COMPLETION_PROMPT_READY` 事件改写提示词的扩展不会参与这条独立组装路径。

## 源码

| 位置 | 职责 |
| --- | --- |
| [frozen-run-input-snapshot.js](../../src/scripts/tauritavern/agent/frozen-run-input-snapshot.js) | 前端冻结输入 |
| [agent-context-policy.js](../../src/scripts/tauritavern/agent/agent-context-policy.js) | 上下文策略投影 |
| [prompt_assembly_service.rs](../../src-tauri/crates/tt-application/src/services/prompt_assembly_service.rs) | 解析预设、模型和组装请求 |
| [agent-prompt-assembly-run.js](../../src/tauri/main/api/agent-prompt-assembly-run.js) | 根 Run 的 prepare/build 编排 |
| [agent-prompt-assembly.js](../../src/tauri/main/api/agent-prompt-assembly.js) | 前端组装器 |
| [openai.js](../../src/scripts/openai.js) | PromptManager 与生成参数 |
| [agent-prompt-assembly-bridge.js](../../src/tauri/main/api/agent-prompt-assembly-bridge.js) | 运行中 Invocation 的组装桥接 |
| [agent_runtime_service/prompt_assembly.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/prompt_assembly.rs) | 等待组装结果与保存快照 |
