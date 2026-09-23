# Prompt assembly

Agent 继续使用 SillyTavern 的 PromptManager 组装提示词。Rust 解析 Profile、预设和模型连接，前端用这些设置生成消息，随后由 Rust 运行模型与工具循环。

## 冻结一次输入

生成准备阶段创建 `FrozenRunInputSnapshot`，保存本次运行需要的输入：

| 内容 | 来源和用途 |
| --- | --- |
| `promptInputs` | 聊天、角色、示例、扩展提示词等 PromptManager 输入 |
| `worldInfoActivation` | 本次生成激活的世界书 |
| `macroContext` | 本次生成的名称、变量、角色与其他宏值 |
| `currentModelConnection` | 本次生成使用的模型连接 |

这份输入供根 Agent、子 Agent 和接手的 Agent 使用。每个 Invocation 可以根据自己的 Profile 决定初始历史范围、提示词和模型。组装在工作副本上进行，保存的输入留作后续组装和诊断。

Session 从自身历史与本次用户消息准备输入，沿用指定预设组装。完整历史保存在 JSONL 中；Run 只保存预算内的最终消息和运行上下文。

## 两条组装路径

默认 Profile 使用当前生成得到的 prompt snapshot。Profile 设置 `preset.mode = ref` 时，按指定预设重新组装：

```text
冻结聊天输入
  → promptAssembly.prepare：Rust 解析 Profile、读取预设、应用连接
  → promptAssembly.buildSnapshot：前端 PromptManager 组装消息
  → startRunWithPromptSnapshot：Rust 创建 Run 并开始循环
```

子 Agent 或交接发生在运行中。它们需要独立预设时，runtime 写入 `prompt_assembly_requested` 事件，host bridge 按请求 ID 读取组装输入，调用同一个前端组装器，再把结果交回等待中的 Invocation。

`agentSystemPrompt` 是 Profile 指令在 PromptManager 中的位置，`agentTask` 是委派或交接任务的位置。Preset 控制组件的位置和 role，runtime 消费最终组装好的消息。

Snapshot 使用 `messages: AgentModelMessage[]` 与独立的 `generationParameters`，旧 Chat payload 在入口适配。

普通 Chat 在 `CHAT_COMPLETION_SETTINGS_READY` 事件修改完成后统一转换 snapshot；SDK 的独立组装不触发该 Chat 事件。

Session 历史按完整的工具调用与结果组参与预算裁剪，保留原始内容和 metadata，不重新展开历史宏。未闭合工具组在下次组装时呈现为中断说明，原历史不改写，工具不重放。实际输入范围由预设预算与 Profile 的历史窗口决定。

## Skill 与 Agent 目录

Runtime 准备 Invocation 时，将 Skill 与可调用 Agent 目录追加到 `agentSystemPrompt` 正文末尾。默认和自定义指令使用同一流程，保留预设的位置与 role；后续轮次和恢复沿用已准备请求。

Skill 目录与文件视图使用同一份有效绑定；Agent 目录由实际工具与目标调用资格决定。规则分别见 [Skill](Skill.md) 和 [多 Agent 协作](SubAgent.md)，手工 snapshot 的组件标记见 [API](../API/Agent.md)。

## 预设与连接各管什么

Preset 提供提示词布局和生成设置。Profile 的 model binding 提供最终 source、model、endpoint、secret 和路由；这些连接字段会覆盖预设中的旧值。

显式的 `preset.reasoningEffort` 优先于预设的参数省略设置；实际值由 PromptManager 按 source 和模型归一化，再由 provider adapter 映射协议。组装不修改保存的预设。

`currentPromptSnapshot` 使用输入中冻结的连接。`connectionRef` 则通过 LLM Connection 解析：组装时应用一次，让 PromptManager 使用正确的模型设置；Invocation 进入循环前再应用到最终请求。Model Target 与反向代理预设的管理见 [LLM Connection API](../API/LlmConnections.md)。

独立组装使用 headless PromptManager，不切换 UI 的预设或模型。动态扩展提示词在输入冻结时收集；依赖普通 `CHAT_COMPLETION_PROMPT_READY` 事件改写提示词的扩展不会参与这条独立组装路径。

## 源码

| 位置 | 职责 |
| --- | --- |
| [frozen-run-input-snapshot.js](../../src/scripts/tauritavern/agent/frozen-run-input-snapshot.js) | 前端冻结输入 |
| [prompt_assembly_service.rs](../../src-tauri/crates/tt-application/src/services/prompt_assembly_service.rs) | 解析预设、模型和组装请求 |
| [agent-prompt-assembly-run.js](../../src/tauri/main/api/agent-prompt-assembly-run.js) | 根 Run 的 prepare/build 编排 |
| [agent-prompt-assembly.js](../../src/tauri/main/api/agent-prompt-assembly.js) | 前端组装器 |
| [openai.js](../../src/scripts/openai.js) | PromptManager 与生成参数 |
| [agent-prompt-assembly-bridge.js](../../src/tauri/main/api/agent-prompt-assembly-bridge.js) | 运行中 Invocation 的组装桥接 |
| [agent_runtime_service/prompt_assembly.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/prompt_assembly.rs) | 等待组装结果与保存快照 |
