# 多 Agent 协作

多个 Agent 在同一个 Run 中工作。每个 Agent 的一次执行是一个 Invocation，有自己的 Profile、上下文和工具循环；文件保存在共享工作区中。

协作通过任务包传递目标和材料，通过结果包传回结论。它类似管道中的输入与输出，runtime 用任务记录连接两端，文件路径承载较大的材料。

## 委派与交接

两种协作方式共用 `AgentTaskRecord`，区别在于任务之后由谁继续：

| 方式 | 工具 | continuation | 后续工作 |
| --- | --- | --- | --- |
| 委派 | `agent.delegate` | `return_to_parent` | 子 Agent 返回结果，调用方继续 |
| 交接 | `agent.handoff` | `transfer_control` | 当前 Agent 退出，下一个 Agent 接手 |

任务记录保存调用方、接收方、目标 Profile、任务内容和状态。模型只需选择 Agent、描述任务并引用文件；Invocation ID 和任务文件路径由 runtime 管理。

## 委派一个任务

调用方用 `agent.list` 查看可用 Agent，然后交付任务：

```json
{
  "agentId": "reviewer",
  "task": {
    "objective": "检查人物动机是否前后一致，并将修改建议写入 summaries/review.md。",
    "draft": "output/main.md"
  }
}
```

`task.objective` 必填，`title` 可选，其他字段可以携带这项工作需要的材料。`agent.delegate` 返回任务 ID 后，调用方可以继续做其他事。需要结果时调用 `agent.await`；它支持等待一个完成、等待全部完成或只读状态。

子 Agent 使用 `task.return` 结束：

```json
{
  "summary": "发现两处动机衔接问题，修改建议已写入 summaries/review.md。",
  "artifacts": [{ "path": "summaries/review.md", "kind": "markdown", "role": "review" }]
}
```

runtime 保存结构化结果，并生成 `summaries/<workspace-key>-result.md` 供其他 Agent 读取。调用方收到摘要、文件引用及结果中的其他字段，随后决定如何使用。已完成结果也会在后续轮次送入调用方上下文。

委派任务使用后台执行和 `task.return` 结束方式，聊天提交由前台 Agent 负责。当前 return-mode 子 Agent 不再向下委派或交接。

## 交给下一个 Agent

适合把资料整理、起草、润色等阶段交给不同 Profile：

```json
{
  "agentId": "editor",
  "handoff": {
    "objective": "完成最后一轮润色并提交正文。",
    "contextSummary": "事实核对已完成，保留原有叙述视角。",
    "workspaceRefs": ["output/main.md", "summaries/review.md"]
  }
}
```

`handoff.objective` 必填，其余内容按任务需要填写。交接前先收齐当前 Agent 的委派任务。交接成功后，当前 Invocation 标记为 `transferred`，executor 在同一 Run 中启动接收方；接收方沿用已有文件和提交记录，通过 `workspace.commit`、`workspace.finish` 完成工作。

## 接收方看到什么

runtime 用目标 Profile 准备新的提示词和工具集合。初始聊天材料来自 Run 启动时冻结的输入，任务包通过 `agentTask` 进入提示词。调用方应在任务中写明目标、必要背景和文件路径。

共享工作区意味着双方看到同一路径的内容。Profile 决定各自能读写哪些目录；涉及同一文件的并行工作仍使用工作区的内容冲突检查。不同任务可以约定各自输出文件，再由调用方汇总。

Profile 的 `delegation` 配置控制是否可被调用、是否可委派或交接、调用来源与并发数量。返回式子任务由 scheduler 运行；Run 收尾或取消时，runtime 结束尚未完成的子任务。

## 查看与修改实现

Timeline 使用 Invocation 和 Task 记录展示关系，用日志显示每个 Agent 的过程。`readTaskDetail({ runId, taskId, includeResult })` 按 ID 读取任务及按需结果。

- [invocation.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/invocation.rs)：创建 Invocation 与 Task。
- [delegation](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/delegation)：委派、等待、返回、交接及接收方提示词。
- [scheduler.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/scheduler.rs)：后台子任务与取消。
- [descriptors.rs](../../src-tauri/crates/tt-application/src/services/agent_tools/agent/descriptors.rs)：模型可见工具参数。
