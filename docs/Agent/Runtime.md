# 运行循环

一次 Run 从冻结输入开始，在 Invocation 中执行模型与工具循环。`target` 明确区分 Chat 与 Session：Chat 完成文件和聊天提交，Session 持续保存对话并直接答复。

## 启动

前端生成入口读取当前聊天、世界书激活结果、宏和连接设置，形成 `FrozenRunInputSnapshot`。PromptManager 根据 Profile 与预设生成初始消息，Host API 把结果交给 `AgentRuntimeService`。

Runtime 创建 Run，初始化工作区，保存输入和解析后的 Profile，再准备根 Invocation。此时会确定该 Invocation 的模型请求、工具集合、可用 Skill、可调用 Agent 目录与工作区范围。后续修改 UI 设置不会改写已经准备好的 Invocation。

`startRunFromLegacyGenerate()` 负责从当前聊天取得输入；`startRunWithPromptSnapshot()` 接受已经组装好的输入。两者进入相同的 Rust runtime。具体参数见 [API](../API/Agent.md)，提示词的生成过程见 [Prompt assembly](PromptAssembly.md)。

`sessions.send()` 每次创建一个 Run，共用该 Session 的历史与工作区。同一 Session 只允许一个活跃 Run；准备期间历史发生变化时拒绝启动。

## 每一轮

每个 Invocation 在累计预算内交替调用模型与工具：

1. Chat 前台消费待处理的用户补充指令；记录本轮模型请求。
2. 通过 Agent Model Gateway 调用模型，保存响应并将完整 assistant 消息加入请求历史。
3. 按返回顺序执行工具，将已确认的结果加入上下文。
4. 本轮工具全部结算后才进入下一次模型请求。

同一轮中后面的调用可以依赖前面调用的效果。Chat 的典型回复：写入正文，需要时更新 `persist/`，再以 `workspace.commit`（`finish: true`）提交并结束；合并提交与结束省去单独结束的一轮。

委派结果也在轮次之间进入调用方上下文。工具参数等可由模型修正的问题作为 tool result 返回；模型请求、存储或运行状态错误交给 Run 收尾处理。模型某一轮没有工具调用、只回了文字时，按[结束策略](#结束与交接)处理：条件满足就结束 Run，否则提醒模型。提醒时，Chat runtime 会把正文保存为文件，并在剩余轮数内提示它以 `finish: true` 提交该文件（或改写后提交）；runtime 不代为提交，`persist/` 的更新仍由模型决定。

Session 在 assistant 无工具调用时结束本轮，消息由后端连续保存。取消保留已写入的消息和工作文件。

流式调用提供正文、思考及工具参数预览，正式消息的关联规则见 [Agent API](../API/Agent.md#控制与订阅)。工具执行始终使用完整响应；重试策略只作用于模型请求，工具执行由本轮调用记录管理。

## 结束与交接

Run 有两种结束方式：带 `finish: true` 的 `workspace.commit`，或只回文字的一轮满足结束策略。

带 `finish: true` 的提交在宿主确认后结束 Run，提交被拒绝则不结束。它与 `agent.handoff`、`task.return` 一样须是本轮最后一个调用。同一轮中较早的调用返回错误时，提交照常完成，但 Run 不结束，结果里带 `agent.finish_after_failed_call`，使模型先看到该错误。这项检查只用于 `workspace.commit` 的 `finish: true`，`agent.handoff` 与 `task.return` 没有，较早的调用失败时仍照常交接或返回。能交接的阶段不直接结束 Run，使用 `finish: true` 时返回可恢复错误；其他阶段都能结束。前台 Chat 中能结束 Run 的阶段须有 `workspace.commit`，交接接收方也按此检查；后台运行可以不写聊天。文件发布和持久版本的关系见 [Workspace](Workspace.md)。

结束策略决定只回文字的一轮怎么处理，是结束规则的唯一来源：runtime 的判断、提示词、续行与委派提示、工具结果文字和上面的 Chat 准入都从 `FinishPolicy`（`agent_tools/finish_policy.rs`）推出。它按 Run 的 presentation 取值，交接接收方沿用 Run 的 presentation，不看自身 Profile 保存的值。默认值在 `FinishPolicy::for_stage`：

| 阶段 | 策略 |
|---|---|
| 前台 Chat 中能结束 Run 的阶段 | 有条件 `[committed]`：楼层显示的是宿主确认的发布就结束，否则提醒模型提交。本次 Run 的显式提交或自动提交都算；修订从已完成的回复开始，算作已确认。之后被流式预览覆盖或有提交被拒，就不再算，见 [Workspace](Workspace.md#提交到聊天) |
| 后台 Chat 中能结束 Run 的阶段 | 有条件 `[]`：模型停下就结束 |
| return-mode 子 Agent、能交接的阶段 | 提醒，行为不变 |

按策略结束时记录 `agent_loop_finished`（`endedBy: "text_turn"`）；文字留在 transcript 里，不发布到聊天。条件目前只有 `committed`，计划完成（`plan_complete`）留待以后加入。前台阶段的说明文字引导模型以带 `finish: true` 的提交结束，纯文字结束只作兜底。

已知限制：根 Invocation 的提示词由前端在 Run 创建前准备，只能按 Profile 保存的 presentation 写结束说明。启动时传入不同的 `presentation`（例如聊天入口固定为前台）时，提示词与 runtime 的判断可能不一致；计划在后续 PR 让准备阶段接收启动时的 presentation。

return-mode 子 Agent 使用 `task.return` 结束，把结果交给调用方。`agent.handoff` 则使当前 Invocation 进入 `transferred`，executor 准备下一个 Invocation，继续使用本次 Run 的提交记录。任务机制见 [多 Agent 协作](SubAgent.md)。

Run 正常完成后进入 `completed`。取消进入 `cancelled`；错误发生在已确认聊天提交之后时进入 `partial_success`，此前则进入 `failed`。工作区与日志保留下来，便于查看已有结果和失败位置。

Shell 取消须先等待已启动的文件修改收尾，再记录结果并进入取消终态。

## Checkpoint 与恢复

Chat 每次执行结束时保存 checkpoint，由 runtime 的执行状态与宿主的消息呈现共同构成。中断后的续接沿用原 Run、冻结输入和工作区，保留已确认结果与累计预算。

Session 重启后从已保存历史发起新 Run；当前不支持 resume、revision、运行中 guidance、委派或 handoff。

当前 checkpoint schema 为 2。v1 可只读查看状态，仅已完成的 Run 在首次 `/fix` 时转换；未完成的 v1 Run 需要重新开始。转换保留原权限、Skill 绑定与执行结果，将含退役工具调用的轮次转为历史文本，并通过修订说明补入新用法与目录。原始记录不改写，旧调用不重放。

`/fix 修改要求` 用于修改最后一条已完成 Agent 回复的当前 swipe。当前正文保存到 `output/previous_output.md`，作为包含手工编辑的修改基准。新的前台 Invocation 继承上一前台的上下文，在原 Run 中获得正常预算；原提交记录与工作材料继续保留。

恢复时仅按需校验所选 Run 的本地保存材料，不依赖同步状态或其他 Run。该 Run 在本进程中仍活跃、材料缺失或不匹配、外部副作用无法确认时拒绝本次恢复；可恢复的保存错误允许重试。恢复与清理须互斥，已确认的工具和提交效果不得重放。

此能力用于已结束执行的续接，不提供进程崩溃时的任意位置恢复。Checkpoint 随运行材料传输和清理。调用方式见 [Agent API](../API/Agent.md)，跨设备恢复的范围见 [同步](../CurrentState/Sync.md)。

## 修改代码从哪里开始

以下路径相对于 `src-tauri/crates/tt-application/src/services/`：

| 位置 | 职责 |
| --- | --- |
| [agent_runtime_service/lifecycle.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/lifecycle.rs) | 输入校验、创建与取消 Run |
| [agent_runtime_service/session.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/session.rs) | Session 配置、历史读取、发送受理与消息记录 |
| [agent_runtime_service/executor.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/executor.rs) | 准备根 Invocation、推进交接链、处理终态 |
| [agent_runtime_service/checkpoint.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/checkpoint.rs) | Checkpoint 保存、读取与恢复准入 |
| [agent_runtime_service/revision.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/revision.rs) | 已完成输出的后续修订 |
| [agent_runtime_service/loop_runner.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/loop_runner.rs) | 模型与工具循环 |
| [agent_runtime_service/tool_execution.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/tool_execution.rs) | 工具调用、结果与副作用记录 |
| [agent_runtime_service/commit.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/commit.rs) | 聊天提交与 Run 收尾 |
| [agent_runtime_service/guidance.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/guidance.rs) | 运行中的用户补充指令 |

前端启动与宿主桥接位于 [src/tauri/main/api/agent.js](../../src/tauri/main/api/agent.js)，界面位于 [agent-system](../../src/scripts/extensions/agent-system/src)。
