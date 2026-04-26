# TauriTavern Agent Run Event Journal

本文档定义 Agent Run 的事件日志、状态机、订阅、恢复、取消与审批语义。

Run Journal 是 Agent 系统的真相源。没有 journal 的 Agent 只是一个难以调试的异步流程。

## 1. 原则

1. Append-only：事件只能追加，不能原地修改。
2. Ordered：每个 run 内 event seq 单调递增。
3. Durable：关键副作用前后必须落 journal。
4. Replayable：UI timeline、debug、resume 应尽量从 journal 重建。
5. User-visible：用户能看到工具调用、审批、diff、checkpoint、错误。

## 2. 文件格式

第一期建议 JSONL：

```text
events.jsonl
```

每行一个 event envelope：

```json
{
  "seq": 12,
  "id": "evt_...",
  "runId": "run_...",
  "timestamp": "2026-04-26T00:00:00Z",
  "level": "info",
  "type": "tool_call_completed",
  "payload": {},
  "causality": {
    "parentEventId": "evt_...",
    "requestId": "model_req_..."
  }
}
```

要求：

- `seq` 由 repository 分配。
- `id` 全局唯一或 run 内唯一均可，但必须稳定。
- `type` 使用 snake_case。
- payload 必须可反序列化为 tagged enum。
- 大文本、二进制、长 tool result 不直接塞进 event，使用 resource ref。

## 3. Run Status

Phase 2A 当前已落地状态：

```text
Created
InitializingWorkspace
AssemblingContext
CallingModel
DispatchingTool
ApplyingWorkspacePatch
CreatingCheckpoint
AssemblingArtifacts
AwaitingCommit
Committing
Completed
Cancelling
Cancelled
Failed
```

后续规划状态：

```text
Created
InitializingWorkspace
AssemblingContext
Planning
Running
CallingModel
AwaitingApproval
DispatchingTool
ApplyingWorkspacePatch
CreatingCheckpoint
AssemblingArtifacts
Committing
Completed
Cancelling
Cancelled
Failed
```

终态：

```text
Completed
Cancelled
Failed
```

状态迁移必须通过 event 记录，例如：

```text
status_changed { from, to, reason }
```

## 4. Event 类型

Phase 2A 当前实际写入的主要事件：

```text
run_created
generation_intent_recorded
status_changed
workspace_initialized
context_assembled
model_request_created
model_completed
tool_call_requested
tool_call_started
tool_result_stored
tool_call_completed
tool_call_failed
workspace_file_written
checkpoint_created
agent_loop_finished
artifact_assembled
commit_started
commit_draft_created
run_committed
run_completed
run_cancel_requested
run_cancelled
run_failed
```

以下小节同时包含 Phase 2A 已落地事件和后续阶段设计事件；实现新事件时必须更新 `docs/CurrentState/AgentFramework.md`。

### 4.1 Run Lifecycle

```text
run_started
status_changed
run_completed
run_cancel_requested
run_cancelled
run_failed
```

`run_failed` payload：

```json
{
  "code": "tool_policy_denied",
  "message": "Tool mcp.foo is not allowed by current plan node",
  "retryable": false,
  "details": {}
}
```

### 4.2 Workspace

```text
workspace_initialized
workspace_file_written
workspace_patch_requested
workspace_patch_applied
workspace_patch_failed
workspace_file_deleted
workspace_rollback_completed
```

Workspace event 不应内联大文件全文。应记录 path、sha256、bytes、patch ref。

### 4.3 Context

```text
context_assembly_started
context_component_added
context_component_skipped
context_assembled
context_assembly_failed
```

`context_component_skipped` 必须有 reason，例如 budget、policy hidden、empty。

Policy 拒绝不是 skipped，而是 failure。

### 4.4 Model

```text
model_request_created
model_request_sent
model_delta
model_tool_call_delta
model_completed
model_failed
```

`model_request_created` 应记录 request ref、profile id、provider/source、model、token estimate，不应默认记录完整 prompt。完整 prompt 是否保存取决于调试设置与隐私策略。

### 4.5 Tool

```text
tool_call_requested
tool_call_awaiting_approval
tool_call_approved
tool_call_denied
tool_call_started
tool_call_completed
tool_call_failed
```

`tool_call_requested`：

```json
{
  "callId": "call_...",
  "toolName": "workspace.apply_patch",
  "displayName": "Apply patch",
  "argumentsRef": "events/blobs/call_..._args.json",
  "approvalRequired": false,
  "policy": {
    "source": "plan_node",
    "allowed": true
  }
}
```

`tool_call_completed`：

```json
{
  "callId": "call_...",
  "toolName": "workspace.apply_patch",
  "resultRef": "tool-results/call_....json",
  "isError": false,
  "durationMs": 120,
  "usage": {}
}
```

### 4.6 Checkpoint / Diff

```text
checkpoint_created
checkpoint_pruned
diff_created
```

Checkpoint event must include checkpoint id、reason、file count、bytes。

### 4.7 Plan / Profile

```text
plan_created
plan_updated
plan_node_started
plan_node_completed
plan_policy_violation
profile_selected
profile_switch_requested
profile_switched
profile_switch_denied
```

Locked plan violation 必须失败或等待用户决策，不能静默忽略。

### 4.8 Artifact / Commit

```text
artifact_assembly_started
artifact_assembled
artifact_assembly_failed
commit_started
run_committed
commit_failed
committed_message_rollback_completed
```

Commit event must include chat ref、message index/id、checkpoint id、artifact set id。

## 5. 事件与副作用的顺序

推荐顺序：

```text
意图事件
  -> 执行副作用
  -> 结果事件
```

例如 tool call：

```text
tool_call_requested
tool_call_started
tool dispatch
tool_call_completed / tool_call_failed
```

例如 workspace patch：

```text
workspace_patch_requested
apply patch
workspace_patch_applied
checkpoint_created
```

如果副作用前需要保证恢复后不会重复执行，可以先写 pending event，再由恢复逻辑检查 pending 状态。这一点对 MCP/外部副作用尤其重要。

## 6. 实时订阅

前端订阅 API：

```js
const unsubscribe = await window.__TAURITAVERN__.api.agent.subscribe(runId, event => {});
```

要求：

- subscribe 不复播全部历史，除非 options 指定。
- UI 首次进入 run 页面应先 `readEvents(runId, { afterSeq })`，再 subscribe。
- unsubscribe 必须幂等。
- 事件丢失时，UI 可通过 `afterSeq` 补拉。

## 7. 分页读取

Journal 读取需要支持：

```text
readEvents(runId, { afterSeq, limit })
readEvents(runId, { beforeSeq, limit })
```

移动端 timeline 不应该一次读取巨大 journal。

## 8. Cancel

Cancel 是用户意图，不是 failure。

流程：

```text
cancel_agent_run(runId)
  -> run_cancel_requested
  -> signal cancellation token
  -> 当前可取消操作停止
  -> run_cancelled
```

约束：

- LLM call 必须尽量复用现有 cancellation registry 或等价 watch channel。
- Tool dispatch 需要声明是否 cancellable。
- Cancel 后不能 commit。
- Cancel 后 workspace 与 checkpoint 保留。

## 9. Approval

危险工具、MCP tool、commit、profile switch 可以要求审批。

流程：

```text
tool_call_requested
tool_call_awaiting_approval
approveToolCall({ approved: true/false })
tool_call_approved / tool_call_denied
```

审批必须记录：

- requested tool
- arguments summary/ref
- policy reason
- user decision
- decision timestamp

审批拒绝不是系统错误。它可以进入模型可见 tool error，或让 run 暂停/失败，取决于 plan policy。

## 10. Resume

第一期可以不实现自动 resume，但 journal 设计必须支持。

Resume 的基本策略：

- 读取最后状态。
- 如果终态，拒绝 resume。
- 如果 pending model call，没有 result event，标记 previous attempt interrupted，重新发起或让用户选择。
- 如果 pending external tool call，默认不重复执行，要求人工确认。
- 如果 pending workspace patch，检查 patch result/ref 决定是否重放。

外部副作用必须谨慎，不能因为恢复而重复调用付费 API 或危险工具。

## 11. Error 分类

建议错误 code 分层：

```text
agent.invalid_intent
agent.invalid_profile
agent.policy_violation
agent.cancelled
workspace.path_denied
workspace.required_artifact_missing
workspace.patch_failed
context.budget_exceeded
model.provider_denied
model.request_failed
tool.not_found
tool.policy_denied
tool.execution_failed
mcp.server_denied
mcp.tool_denied
commit.cursor_integrity
commit.save_failed
journal.append_failed
```

`journal.append_failed` 是严重错误。没有 journal 就不能继续执行副作用。

## 12. MVP Event Set

Phase 1 至少实现：

```text
run_started
status_changed
workspace_initialized
context_assembled
model_request_created
model_completed
workspace_file_written
checkpoint_created
artifact_assembled
commit_started
run_committed
run_cancel_requested
run_cancelled
run_failed
```

这套最小事件已经足够支撑 one-step run、timeline、cancel、debug 和 rollback 基础。
