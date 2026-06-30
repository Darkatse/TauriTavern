# TauriTavern Agent Handoff Runtime

本文档记录当前 `agent.handoff` 的实现基线、核心流程、数据结构、模型可见语言边界与代码定位。后续开发 handoff 链、profile routing、task cancel 或多 Agent timeline 前，应先读本文，再读 `SubAgent.md`、`ToolSystem.md` 与 `PromptAssembly.md`。

当前状态截至 2026-06-06：handoff MVP 已落地。root / handoff foreground owner 可以把同一个 `AgentRun` 的下一阶段交给目标 Agent。handoff 使用 `AgentTaskRecord + AgentInvocation` 的统一 delegation 模型，但不进入后台 scheduler，也不会把结果返回给调用方；executor 会在当前 invocation 转为 `Transferred` 后继续运行目标 `Handoff` invocation。

## 1. 设计目标

Handoff 解决的是写作协作中的接力问题：

```text
root writer drafts and commits
  -> handoff to editor
editor revises and commits
  -> handoff to finalizer
finalizer verifies, commits, finish
```

它不同于 return-mode SubAgent：

| 能力 | `agent.delegate` | `agent.handoff` |
| --- | --- | --- |
| 模型意图 | 请另一个 Agent 做一个任务，并把结果交还给你 | 请另一个 Agent 接手下一阶段 |
| continuation | `ReturnToParent` | `TransferControl` |
| 执行方式 | run-scoped scheduler 后台 worker | executor 前台串接 |
| 调用方后续 | 可继续工作，可 `agent.await` | 成功后当前 invocation 结束为 `Transferred` |
| 结果回流 | `task.return` result capsule | 不回流给调用方 |
| chat commit | child 不可 commit | handoff owner 可 commit / finish |

对 TauriTavern 来说，handoff 的价值不是抢占式调度，而是让创作者定义不同身份、提示词、preset、模型和工具权限的 Agent，并让这些 Agent 按写作阶段接力。已经 commit 过的 Agent 仍允许 handoff；最终 Agent 可以继续修订、再次 commit 并 `workspace.finish`。

## 2. 核心不变量

- Handoff 仍在同一个 `AgentRun` 内运行，共享 run workspace、journal、cancel、checkpoint、commit ledger 与 persistent projection。
- 每个 handoff stage 是独立 `AgentInvocation`，拥有自己的 provider session id、target Profile、工具面、Skill 解析与 prompt assembly。
- `agent.handoff` 是模型可见工具，`TransferControl` 是 runtime 内部 continuation。不要把 `continuation`、`invocationId` 或 provider state 暴露给模型参数。
- 当前 Agent 成功 handoff 后必须停止工具调用。runtime 将当前 invocation 记为 `Transferred`。
- Handoff task 不进入 `AgentTaskScheduler`。scheduler 只运行 `ReturnToParent` child tasks。
- Handoff 前当前 invocation 不能有未完成的 return-mode delegated task。MVP 只支持 `pendingTaskPolicy = denyIfPending`。
- Handoff owner 使用 `RunFinishAllowed`，可根据 Profile 工具面继续 `workspace.commit` / `workspace.finish` / `agent.handoff`。
- Tool result 不写入 SillyTavern chat 楼层。最终聊天输出仍只通过 `workspace.commit` 进入 host commit bridge。

## 3. 模型可见工具

当前 `agent.handoff` ToolSpec 位于：

```text
src-tauri/src/application/services/agent_tools/agent/specs.rs
```

模型看到的是调用 Agent 视角的工具：

```json
{
  "agentId": "final-editor",
  "handoff": {
    "title": "Final revision pass",
    "reason": "I have drafted and committed the main scene; a revision-focused Agent should polish it.",
    "objective": "Revise output/main.md for rhythm, dialogue flow, and final coherence, then commit and finish when ready.",
    "contextSummary": "The draft has been written and committed once. Keep the existing plot beats, but improve prose and pacing.",
    "workspaceRefs": ["output/main.md", "persist/story_state.md"],
    "mustPreserve": [
      "Do not change the core plot outcome.",
      "Keep the established character voice."
    ],
    "completionCriteria": [
      "output/main.md is polished.",
      "The final chat message has been committed.",
      "The run is finished if no further Agent is needed."
    ]
  },
  "pendingTaskPolicy": "denyIfPending"
}
```

字段说明：

- `agentId`：目标 Agent id，通常来自 `agent_list({ "purpose": "handoff" })`。
- `handoff`：传给下一个 Agent 的 brief。它是模型可见协作协议，不是 runtime packet。
- `handoff.objective`：唯一必填字段，必须是非空字符串。
- `handoff.title` / `reason` / `contextSummary`：可选字符串，提供时会校验长度。
- `handoff.workspaceRefs` / `mustPreserve` / `completionCriteria`：schema 引导字段，renderer 会按 markdown section 渲染。当前 runtime 不对它们做强类型校验，以保留创作者扩展空间。
- 其它 `handoff` 字段：允许存在，会渲染到 `Additional Instructions`。新增字段前仍应遵守 Agent-friendly 原则，确认它是下一个 Agent 完成任务必须知道的信息。
- `pendingTaskPolicy`：当前只支持 `denyIfPending`。省略时默认 `denyIfPending`。

成功 tool result 的模型可见 content 是轻量确认：

```text
Handoff accepted for Agent final-editor. Your part is complete.
```

该结果通常不会再进入当前 Agent 的下一轮模型思考，因为 loop runner 收到 `HandoffAccepted` effect 后会结束当前 invocation。

## 4. Runtime 存储结构

`agent.handoff` 参数先反序列化为内部 `AgentHandoffArgs`：

```rust
struct AgentHandoffArgs {
    agent_id: String,
    handoff: Value,
    pending_task_policy: Option<PendingTaskPolicy>,
}
```

校验通过后，runtime 创建一个 `AgentTaskRecord` 和一个后继 `AgentInvocation`。

Handoff task 形态：

```json
{
  "id": "handoff_<uuid>",
  "runId": "<run-id>",
  "parentInvocationId": "<current-invocation-id>",
  "childInvocationId": "inv_<uuid>",
  "targetProfileId": "final-editor",
  "workspaceKey": "final-editor",
  "continuation": "transfer_control",
  "status": "queued",
  "task": {
    "title": "Final revision pass",
    "objective": "Revise output/main.md...",
    "contextSummary": "...",
    "workspaceRefs": ["output/main.md"]
  },
  "createdByToolCallId": "<tool-call-id>",
  "resultRef": null,
  "error": null
}
```

后继 invocation 形态：

```json
{
  "id": "inv_<uuid>",
  "runId": "<run-id>",
  "parentInvocationId": "<current-invocation-id>",
  "profileId": "final-editor",
  "kind": "handoff",
  "status": "created",
  "exitPolicy": "run_finish_allowed"
}
```

`childInvocationId` 是统一 delegation model 的字段名。对 handoff 来说，它表示 successor invocation，而不是 return-mode child worker。不要为了语义纯度单独复制一套 handoff record；当前统一模型能保持 repository、journal、prompt assembly 与 timeline 复用。

## 5. 执行流程

### 5.1 Tool dispatch

入口：

```text
src-tauri/src/application/services/agent_runtime_service/delegation/handoff_tool.rs
```

流程：

```text
current Agent calls agent_handoff
  -> parse AgentHandoffArgs
  -> validate handoff packet
  -> default pendingTaskPolicy to denyIfPending
  -> event agent_handoff_requested
  -> check source can_handoff
  -> check no pending ReturnToParent child tasks
  -> check max_handoff_depth
  -> parse and resolve target profile
  -> validate target callable / allowAsHandoffTarget / allowedCallers / model configured
  -> create TransferControl task + Handoff invocation
  -> event agent_handoff_accepted
  -> return AgentToolEffect::HandoffAccepted
```

当前 pending child 检查只看 `continuation = ReturnToParent` 且状态为 `Queued | Running` 的 task。Handoff task 本身不属于这个阻塞集合。

### 5.2 Loop exit

`run_tool_loop()` 收到 `AgentToolEffect::HandoffAccepted` 后：

```text
record handoff task id + new invocation id
finish current invocation as Transferred
emit agent_loop_finished
return AgentLoopExit::Transferred { task_id, new_invocation_id }
```

实现位置：

```text
src-tauri/src/application/services/agent_runtime_service/loop_runner.rs
```

### 5.3 Executor 串接

`execute_active_invocation_chain()` 处理 `AgentLoopExit::Transferred`：

```text
if current invocation itself came from an incoming handoff task:
  mark that incoming task Completed

prepare_handoff_invocation(task_id, new_invocation_id)
  -> transition handoff task to Running
  -> start new invocation
  -> resolve target profile
  -> assemble context for target invocation

continue loop with:
  invocation_id = new_invocation_id
  request = target request
  profile = target profile
  effective_skills = target effective skills
```

实现位置：

```text
src-tauri/src/application/services/agent_runtime_service/executor.rs
src-tauri/src/application/services/agent_runtime_service/delegation/child_runtime.rs
```

当最终 handoff owner 调用 `workspace.finish` 并成功收尾时，executor 会把它的 incoming handoff task 标记为 `Completed`。如果 handoff owner 再次 handoff，旧 incoming handoff task 也会先标记为 `Completed`，然后继续下一段 handoff invocation。

## 6. 下一个 Agent 收到什么

下一个 Agent 不直接看到裸 JSON。`handoff` Value 会通过 `render_handoff_task_prompt()` 渲染为 markdown brief：

```markdown
# Handoff Brief

You are now responsible for the next stage of this run.
Continue from the shared workspace paths and constraints below.

## Title
Final revision pass

## Reason
I have drafted and committed the main scene; a revision-focused Agent should polish it.

## Objective
Revise output/main.md for rhythm, dialogue flow, and final coherence, then commit and finish when ready.

## Context Summary
The draft has been written and committed once. Keep the existing plot beats, but improve prose and pacing.

## Workspace References
- output/main.md
- persist/story_state.md

## Must Preserve
- Do not change the core plot outcome.
- Keep the established character voice.

## Completion Criteria
- output/main.md is polished.
- The final chat message has been committed.
- The run is finished if no further Agent is needed.

## Working Notes
- Inspect the referenced workspace files before editing existing content.
- Preserve previous decisions and committed text unless this brief asks you to revise them.
- If commit and finish tools are available to you, use them only when this run is ready to end.
- If another Agent should continue after your stage, hand off with a clear brief.
```

渲染入口：

```text
src-tauri/src/application/services/agent_runtime_service/delegation/rendering.rs
```

Prompt assembly 路径：

- target Profile `preset.mode = "ref"`：runtime 注册 invocation-scoped `prompt_assembly_requested`，前端 PromptAssemblyBroker 使用 target Profile 的 preset、system prompt、`agentTaskPrompt` 与 frozen input 重新组装。
- target Profile `preset.mode = "currentPromptSnapshot" | "none"`：后端兼容路径使用 root run 的 `input/prompt_snapshot.json`，替换为 target Profile 的 materialized system prompt + handoff brief。

两条路径都会：

- 应用 target Profile 的 model binding。
- 注入 target Profile 可见工具。
- 使用 `provider_state.sessionId = runId:invocationId`。
- 按 target Profile 解析 Skill scope，并写入 `input/invocations/<invocationId>/resolved_skills.json`。

## 7. Profile 与工具面策略

Source Agent 要能 handoff：

- `delegation.canHandoff = true`
- `tools.allow` 显式包含 `agent.handoff`
- 当前 invocation 是 root / active foreground owner，而不是 return-mode child

Target Agent 要能接手：

- `delegation.callable = true`
- `delegation.allowAsHandoffTarget = true`
- `delegation.allowedCallers` 允许 source Profile id 或 `*`
- model binding 已配置，不是 unresolved `requiresConfiguration`

Target handoff invocation 使用自己的 Profile 工具面。常见配置：

- 最终收尾 Agent：暴露 `workspace.commit` + `workspace.finish`。
- 中间修订 Agent：可暴露 `workspace.commit` + `agent.handoff`，允许先提交阶段性文本再交给下一位。
- 只接力不收尾 Agent：可以没有 `workspace.finish`，但必须有后续可用的 handoff 路径，否则模型会没有正常结束工具。

默认 system prompt 会根据实际工具面提示：

- 有 `workspace.finish`：不要 plain text answer，最终通过 finish 收口。
- 没有 `workspace.finish` 但有 `agent.handoff`：完成自己的部分后调用 handoff。
- 有 `agent.handoff`：handoff 成功后不要继续调用工具。

## 8. Commit 与 finish 语义

Handoff 链共享同一个 `RunCommitLedger`。因此：

- 前一个 Agent 已经 `workspace.commit` 后，后续 Agent 可以继续修订并再次 commit。
- `workspace.finish` 的前台 commit 要求是 run-level，不是 invocation-local。只要同一个 run 已经有成功 commit，后续 handoff owner 可以 finish。
- 仍建议最终 Agent 在做实质修订后再次 commit，避免用户看到的是上一个阶段的输出。
- `workspace.finish` 当前会取消当前 parent 拥有的 unfinished return-mode child tasks，并在 run 收尾时取消剩余 unfinished child tasks。handoff task 代表 active chain，不进入这类默认取消集合。

## 9. Event 序列

一次成功 handoff 的典型 journal 序列：

```text
agent_handoff_requested
agent_invocation_created           # kind = handoff
agent_task_registered              # continuation = transfer_control
agent_handoff_accepted
agent_invocation_transferred       # source invocation
agent_loop_finished                # source invocation loop ends
agent_task_started                 # handoff task transitions Running
agent_invocation_started           # target invocation starts
prompt_assembly_requested?         # only when target preset.mode = ref
context_assembled
skill_scopes_resolved
...
agent_task_completed               # incoming handoff task completed when target finishes or hands off again
agent_invocation_completed         # final owner finish path
run_completed
```

Timeline UI 不应只依赖当前分页内的 journal 事件推断 active chain。`readEvents({ includeTimelineProjection: true })` 会随事件页返回 `timelineProjection.foregroundInvocationIds`、`timelineProjection.invocations` 与 `timelineProjection.delegationEdges`；该 projection 来自结构化 invocation/task repository，用于在 `agent_handoff_accepted`、`agent_delegate_started` 或 `agent_invocation_started` 已不在当前页时仍展示同一 run 内的 Agent graph。projection 不是新的 journal event，不能作为审计日志来源；普通 event polling 不应请求它。

失败和拒绝路径：

- 参数错误、policy denied、pending delegated tasks、depth exhausted：返回 recoverable tool error 给当前 Agent，run 不立即失败。
- 创建 task / invocation、repository、journal、prompt assembly、provider state、serialization 等宿主契约错误：fail-fast，按 active invocation 失败处理。
- target handoff invocation 启动失败：`mark_active_invocation_failed()` 会标记新 invocation，并把 handoff task 标记为 failed。

## 10. 代码定位

模型可见工具：

```text
src-tauri/src/application/services/agent_tools/agent/specs.rs
src-tauri/src/application/services/agent_tools/agent/mod.rs
src-tauri/src/application/services/agent_tools/registry.rs
src-tauri/src/application/services/agent_tools/dispatcher.rs
```

Handoff dispatch / policy / rendering：

```text
src-tauri/src/application/services/agent_runtime_service/delegation/handoff_tool.rs
src-tauri/src/application/services/agent_runtime_service/delegation/policy.rs
src-tauri/src/application/services/agent_runtime_service/delegation/rendering.rs
src-tauri/src/application/services/agent_runtime_service/delegation/child_runtime.rs
```

Invocation / executor / loop：

```text
src-tauri/src/application/services/agent_runtime_service/invocation.rs
src-tauri/src/application/services/agent_runtime_service/loop_runner.rs
src-tauri/src/application/services/agent_runtime_service/executor.rs
src-tauri/src/application/services/agent_runtime_service/tool_execution.rs
```

Prompt assembly：

```text
src-tauri/src/application/services/agent_runtime_service/prompt_assembly.rs
src/tauri/main/api/agent-prompt-assembly-bridge.js
src/tauri/main/api/agent-prompt-assembly.js
```

Profile validation：

```text
src-tauri/src/application/services/agent_profile_service/validation.rs
src-tauri/crates/tt-domain/src/models/agent/profile.rs
```

Tests：

```text
src-tauri/src/application/services/agent_runtime_service/tests.rs
src-tauri/src/application/services/agent_profile_service/tests.rs
src-tauri/src/application/services/agent_tools/registry.rs
```

重点测试名：

```text
agent_handoff_continues_after_prior_commit
agent_handoff_denies_pending_delegated_tasks
default_agent_system_prompt_describes_handoff_from_current_agent_view
agent_tool_specs_keep_runtime_terms_out_of_model_descriptions
handoff_target_profiles_do_not_require_finish_tool
```

## 11. 后续扩展边界

当前实现刻意保持小而完整。后续扩展时优先保持这些边界：

- 不要把 `agent.delegate` 和 `agent.handoff` 合并成带 enum 的单工具。模型可见层应继续保持分离，runtime 内部继续统一。
- 不要让 handoff target 继承 source Agent 的 system prompt、tool surface、Skill policy 或 provider continuation。
- 不要让 handoff 成功后旧 invocation 继续执行副作用工具。
- 不要把 handoff brief 渲染为裸 JSON。目标 Agent 应收到 markdown briefing。
- 不要把 `taskId`、`invocationId`、`profileId`、physical workspace path 或 CAS 细节塞进模型 prompt。
- 如果未来支持 pending child task transfer，应显式设计 task ownership transfer，而不是放宽 `denyIfPending` 静默交接。
- 如果未来支持模型可见 cancel，应先保证 `agent.await`、finish 默认取消与 handoff deny policy 的语义不冲突。
- 如果未来引入 Plan Mode，handoff 应成为 plan edge 的一种实现，而不是绕过 `AgentInvocation` / `AgentTaskRecord` 的独立调度系统。

Agent-facing 文案必须持续从当前 Agent 或目标 Agent 的视角书写。多 Agent 框架是为 Agent 服务的；实现细节只有在 Agent 完成任务必须知道时才进入 prompt、tool description 或 tool result。
