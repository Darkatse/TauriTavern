# 运行循环

一次 Run 从冻结聊天输入开始，在一个或多个 Invocation 中执行模型与工具循环，最后完成文件和聊天的提交。

## 启动

前端生成入口读取当前聊天、世界书激活结果、宏和连接设置，形成 `FrozenRunInputSnapshot`。PromptManager 根据 Profile 与预设生成初始消息，Host API 把结果交给 `AgentRuntimeService`。

Runtime 创建 Run，初始化工作区，保存输入和解析后的 Profile，再准备根 Invocation。此时会确定该 Invocation 的模型请求、工具集合、可用 Skill 与工作区范围。后续修改 UI 设置不会改写已经准备好的 Invocation。

`startRunFromLegacyGenerate()` 负责从当前聊天取得输入；`startRunWithPromptSnapshot()` 接受已经组装好的输入。两者进入相同的 Rust runtime。具体参数见 [API](../API/Agent.md)，提示词的生成过程见 [Prompt assembly](PromptAssembly.md)。

## 每一轮

每个 Invocation 在累计预算内交替调用模型与工具：

1. 将待处理的用户补充指令加入前台上下文，记录模型请求。
2. 通过 Agent Model Gateway 调用模型，保存响应并将完整 assistant 消息加入请求历史。
3. 按返回顺序执行工具，将已确认的结果加入上下文。
4. 本轮工具全部结算后才进入下一次模型请求。

委派结果也在轮次之间进入调用方上下文。工具参数等可由模型修正的问题作为 tool result 返回；模型请求、存储或运行状态错误交给 Run 收尾处理。模型若直接输出正文，runtime 会把正文保存为文件，并在剩余轮数内提示它继续通过工具完成工作。

流式调用额外提供工具参数的实时预览，工具执行仍使用完整的模型响应。重试策略只作用于模型请求；工具执行由本轮调用记录管理。

## 结束与交接

前台 Agent 通过 `workspace.commit` 将文件交给宿主保存，通过 `workspace.finish` 结束。`finish` 要求前台已经完成显式提交；后台运行可以不写聊天。文件发布和持久版本的关系见 [Workspace](Workspace.md)。

return-mode 子 Agent 使用 `task.return` 结束，把结果交给调用方。`agent.handoff` 则使当前 Invocation 进入 `transferred`，executor 准备下一个 Invocation，继续使用本次 Run 的提交记录。任务机制见 [多 Agent 协作](SubAgent.md)。

Run 正常完成后进入 `completed`。取消进入 `cancelled`；错误发生在已确认聊天提交之后时进入 `partial_success`，此前则进入 `failed`。工作区与日志保留下来，便于查看已有结果和失败位置。

## Checkpoint 与恢复

每次执行结束时保存 checkpoint，由 runtime 的执行状态与宿主的消息呈现共同构成。续接沿用原 Run、冻结输入和工作区，保留已确认结果与累计预算；已完成的 Run 只供读取，不再续接。

恢复依赖完整且匹配的保存材料。运行活跃、材料缺失或外部副作用无法确认时拒绝恢复；可恢复的保存错误允许重试。恢复与清理须互斥，已确认的工具和提交效果不得重放。

此能力用于已结束执行的续接，不提供进程崩溃时的任意位置恢复。Checkpoint 随运行材料传输和清理。调用方式见 [Agent API](../API/Agent.md)，跨设备恢复的范围见 [同步](../CurrentState/Sync.md)。

## 修改代码从哪里开始

以下路径相对于 `src-tauri/crates/tt-application/src/services/`：

| 位置 | 职责 |
| --- | --- |
| [agent_runtime_service/lifecycle.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/lifecycle.rs) | 输入校验、创建与取消 Run |
| [agent_runtime_service/executor.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/executor.rs) | 准备根 Invocation、推进交接链、处理终态 |
| [agent_runtime_service/checkpoint.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/checkpoint.rs) | Checkpoint 保存、读取与恢复准入 |
| [agent_runtime_service/loop_runner.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/loop_runner.rs) | 模型与工具循环 |
| [agent_runtime_service/tool_execution.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/tool_execution.rs) | 工具调用、结果与副作用记录 |
| [agent_runtime_service/commit.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/commit.rs) | 聊天提交与 Run 收尾 |
| [agent_runtime_service/guidance.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/guidance.rs) | 运行中的用户补充指令 |

前端启动与宿主桥接位于 [src/tauri/main/api/agent.js](../../src/tauri/main/api/agent.js)，界面位于 [agent-system](../../src/scripts/extensions/agent-system/src)。
