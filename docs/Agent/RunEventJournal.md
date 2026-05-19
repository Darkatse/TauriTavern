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

当前已落地状态：

```text
Created
InitializingWorkspace
AssemblingContext
CallingModel
DispatchingTool
ApplyingWorkspacePatch
CreatingCheckpoint
AwaitingHostCommit
Finishing
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
AwaitingHostCommit
Finishing
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

当前实际写入的主要事件：

```text
run_created
generation_intent_recorded
status_changed
workspace_initialized
persistent_projection_initialized
context_assembled
model_request_created
model_call_attempt_started
model_call_attempt_failed
model_call_retry_scheduled
model_response_stored
provider_state_updated
model_completed
tool_call_requested
tool_call_started
tool_result_stored
tool_call_completed
tool_call_failed
workspace_file_written
workspace_patch_applied
checkpoint_created
context_tool_result_hydrated
agent_loop_finished
artifact_assembled
commit_started
persistent_changes_prepared
persistent_changes_prepare_failed
commit_draft_created
persistent_changes_committed
persistent_changes_commit_failed
run_committed
run_completed
run_cancel_requested
run_cancelled
drift_recovery_attempted
run_rollback_targets
run_failed
```

以下小节同时包含当前已落地事件和后续阶段设计事件；实现新事件时必须更新 `docs/CurrentState/AgentFramework.md`。

### 4.1 Run Lifecycle

```text
run_started
status_changed
run_completed
run_cancel_requested
run_cancelled
drift_recovery_attempted
run_rollback_targets
run_failed
```

`run_failed` payload：

```json
{
  "code": "tool_policy_denied",
  "message": "Tool mcp.foo is not allowed by current plan node",
  "technicalMessage": "Validation error: tool_policy_denied: ...",
  "retryable": false,
  "userRetryable": false,
  "details": {}
}
```

- `retryable`：宿主可不询问用户、安全地自动重试。仅在 `RateLimited`/`Transient` 等暂态错误上为 `true`。
- `userRetryable`：用户可通过 UI 手动重试（前置已 rollback 漂移产物）。`retryable=true` 时一定为 `true`；此外 `model.tool_call_required`、`agent.tool_after_finish`、`agent.max_tool_rounds_exceeded` 等指令漂移类错误也是 `userRetryable=true`，但 **禁止** 自动重试。

当一次 run 因为指令漂移失败、并且该 run 已经通过 `workspace.commit` 向 chat 发布过消息时，loop runner 会在 `run_failed` **之前**额外写一条 `run_rollback_targets` 事件，列出本次 run 留下的"漂移产物"，宿主 UI 应据此回滚这些消息再向用户暴露 Retry：

```json
{
  "reasonCode": "model.tool_call_required",
  "round": 5,
  "targetCount": 1,
  "targets": [
    {
      "path": "output/main.md",
      "mode": "replace",
      "messageId": "10",
      "round": 4
    }
  ]
}
```

**Soft drift recovery**：在直接 fail-fast 之前，loop runner 会先做一次"软纠正"：当模型返回 0 tool_calls 时，把它的纯文本回复推进 history，再追加一条合成的 `user` 消息提醒它必须调用 `workspace_finish`（或 `workspace_apply_patch` + `workspace_finish`），然后让它在下一轮再试一次。每个 run 至多 1 次（受 `DRIFT_RECOVERY_MAX_ATTEMPTS` 控制）。每次尝试都会写一条 `drift_recovery_attempted` 事件，便于宿主 UI 给用户显示"系统正在纠正…"提示：

```json
{
  "attempt": 1,
  "maxAttempts": 1,
  "round": 9,
  "committedCount": 1,
  "reasonCode": "model.tool_call_required"
}
```

- 恢复成功 → run 继续，不会发 `run_rollback_targets`，也不会写 `run_failed`
- 恢复失败（模型再次返回 0 tool_calls）→ 回落到原 #55 路径，发 `run_rollback_targets` + `run_failed`（`userRetryable=true`），允许用户手动重试

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
model_response_stored
provider_state_updated
model_completed
model_failed
```

当前 `model_request_created` 记录 canonical request summary（source、custom format、model、message count、tool count、round），不默认记录完整 prompt。长期应记录 request ref、profile id、provider/source、model、token estimate；完整 prompt 是否保存取决于调试设置与隐私策略。

当前 `model_response_stored` 会把完整 `AgentModelResponse` 写入 `model-responses/round-XXX.json`，event 只记录路径与摘要。`provider_state_updated` 只记录 `provider_state` 摘要字段，不记录完整内部 payload。

`model_completed` 是 UI timeline 的模型回合入口：

```json
{
  "round": 1,
  "modelResponsePath": "model-responses/round-001.json",
  "toolCallCount": 1,
  "textBytes": 26,
  "hasAssistantText": true,
  "assistantTextBytes": 26,
  "hasReasoning": true,
  "reasoningBytes": 30
}
```

前端读取详情时使用 Host ABI `readModelTurn({ runId, round })`，不直接解析 `modelResponsePath` 指向的 raw 文件。

### 4.5 Tool

```text
tool_call_requested
tool_call_awaiting_approval
tool_call_approved
tool_call_denied
tool_call_started
tool_call_completed
tool_call_failed
context_tool_result_hydrated
```

`tool_call_requested`：

```json
{
  "round": 1,
  "callId": "call_...",
  "name": "workspace.apply_patch",
  "argumentsRef": "tool-args/call_....json",
  "providerMetadata": {}
}
```

`tool_call_completed`：

```json
{
  "round": 1,
  "callId": "call_...",
  "name": "workspace.apply_patch",
  "isError": false,
  "errorCode": null,
  "message": null,
  "elapsedMs": 120,
  "resourceRefs": ["output/main.md"]
}
```

`tool_result_stored` 会携带同一 `round` 与 `path`，用于 UI 读取工具结果详情。

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

## 12. 当前核心 Event Set

当前至少应保持：

```text
run_created
generation_intent_recorded
status_changed
workspace_initialized
persistent_projection_initialized
context_assembled
model_request_created
model_response_stored
provider_state_updated
model_completed
tool_call_requested
tool_call_started
tool_result_stored
tool_call_completed
tool_call_failed
workspace_file_written
workspace_patch_applied
checkpoint_created
context_tool_result_hydrated
agent_loop_finished
artifact_assembled
commit_started
persistent_changes_prepared
commit_draft_created
persistent_changes_committed
run_committed
run_completed
run_cancel_requested
run_cancelled
run_rollback_targets
run_failed
```

这套事件已经足够支撑当前 tool loop、timeline、cancel、debug、commit 和后续 rollback/diff 基础。
