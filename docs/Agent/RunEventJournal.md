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
agent_invocation_created
agent_invocation_started
agent_invocation_completed
agent_invocation_failed
agent_invocation_cancelled
agent_invocation_transferred
agent_task_registered
agent_task_queued
agent_task_started
agent_task_completed
agent_task_failed
agent_task_cancelled
agent_delegate_started
agent_handoff_requested
agent_handoff_accepted
agent_await_started
agent_await_completed
task_return_completed
model_request_created
model_call_attempt_started
model_call_attempt_failed
model_call_retry_scheduled
model_response_stored
provider_state_updated
model_completed
direct_output_captured
tool_call_requested
tool_call_started
tool_result_stored
tool_call_completed
tool_call_failed
workspace_file_written
workspace_patch_applied
checkpoint_created
agent_loop_finished
artifact_assembled
commit_started
commit_draft_created
persistent_changes_committed
persistent_changes_commit_failed
persistent_state_metadata_update_requested
persistent_state_metadata_updated
persistent_state_metadata_update_failed
run_committed
run_completed
run_partial_success
run_cancel_requested
run_cancelled
drift_recovery_attempted
run_failed
```

多 invocation 事件必须写入 canonical event scope：

```json
{
  "eventScope": {
    "invocationId": "inv_root",
    "relatedInvocationIds": ["inv_child"]
  }
}
```

`eventScope.invocationId` 是事件主归属 invocation；`eventScope.relatedInvocationIds` 是同一事件关联的其它 invocation。`readEvents({ invocationId })` 先按该 event scope 过滤，再应用 seq 分页。旧 journal 没有 canonical scope 时，后端可以用历史 payload 字段兼容读取，但新事件不应依赖字段名推断作为主契约。

以下小节同时包含当前已落地事件和后续阶段设计事件；实现新事件时必须更新 `docs/CurrentState/AgentFramework.md`。

### Handoff event sequence

当前 `agent.handoff` 的典型事件序列：

```text
agent_handoff_requested
agent_invocation_created      # kind = handoff
agent_task_registered         # continuation = transfer_control
agent_handoff_accepted
agent_invocation_transferred  # source invocation
agent_loop_finished
agent_task_started            # handoff task starts when target invocation is prepared
agent_invocation_started      # target invocation
context_assembled
skill_scopes_resolved
...
agent_task_completed          # incoming handoff task completes when target finishes or hands off again
```

Handoff 的 task / invocation 结构、prompt brief 与失败边界见 `docs/Agent/Handoff.md`。

### 4.1 Run Lifecycle

```text
run_started
status_changed
run_completed
run_partial_success
run_cancel_requested
run_cancelled
drift_recovery_attempted
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

- `retryable`：宿主可不询问用户、安全地自动重试。仅在 `RateLimited`/`Transient` 等暂态错误上为 `true`。例如 `model.upstream_invalid_response` 表示上游响应体本应承载 provider JSON 契约但不可读或不是合法 JSON，属于可重试的 transient invalid response；它不应用于 request build、provider 明确拒绝、tool call / native metadata / response schema 等本地契约错误。
- `userRetryable`：用户可通过 UI 手动重试。`retryable=true` 时一定为 `true`；此外 `model.tool_call_required`、`agent.tool_after_finish`、`agent.max_tool_rounds_exceeded` 等指令漂移类错误也是 `userRetryable=true`，但 **禁止** 自动重试。若 run 已经成功 `workspace.commit` 过 chat，终态会改为 `run_partial_success`，不会复用 `run_failed.userRetryable`，避免用户在已保留输出上直接重试造成重复消息。

`run_partial_success` 是一个独立终态：当一次 run 已经通过 `workspace.commit` 向 chat 发布过至少一条消息，但后续仍因 drift、dispatch、`workspace.finish`、persistent commit 或 persistent metadata 写回错误失败时，executor 会保留这些 host-confirmed chat commit，并写 `run_partial_success`。它不是 clean success：错误仍在 payload 中暴露，`retryable` / `userRetryable` 固定为 `false`，宿主 UI 应以 warning 展示“已保留部分结果”。只有 `persistent_state_metadata_updated` 成功后，chat message 才能带有可作为下一轮 base 的 `persistStateId`。

```json
{
  "code": "model.tool_call_required",
  "message": "model must use Agent tools and finish through workspace_finish",
  "technicalMessage": "Validation error: model.tool_call_required: ...",
  "retryable": false,
  "userRetryable": false,
  "details": {},
  "preservedCommitCount": 1,
  "preservedCommits": [
    {
      "path": "output/main.md",
      "mode": "replace",
      "messageId": "10",
      "round": 4
    }
  ]
}
```

`run_rollback_targets` 作为显式丢弃 / 旧版本清理事件形状保留。它不再是 committed drift 的默认终态路径：自动 rollback 会浪费模型已经通过 host 确认的输出。宿主收到该事件时，rollback 必须 fail-fast：目标缺失、runId 不匹配、rollback strategy 缺失、swipe 状态不安全或宿主删除 API 不可用都必须报错，不能静默跳过或扩大为整条消息删除。若之后暴露 retry 动作，也必须使用 run handle / `generation_intent_recorded` 的 typed generation intent，不得触发 DOM regenerate 按钮。

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

**Soft drift recovery**：在直接 fail-fast 之前，loop runner 会做"软纠正"：当模型返回 0 tool_calls 且包含纯文本时，runtime 会把该文本捕获到当前 profile messageBody artifact 所在 root 的 `direct_output.md`（默认 `output/direct_output.md`），写入 `direct_output_captured` 与 checkpoint；随后把纯文本回复推进 history，再追加一条合成的 `user` 消息提醒它必须通过 Agent 工具继续。root run 如果直接输出就是目标回复，应显式 `workspace_commit` 该捕获文件，再调用 `workspace_finish`；如果需要修订已提交内容，必须先 `workspace_apply_patch` / `workspace_write_file`，再 `workspace_commit`，最后 `workspace_finish`。return-mode child 则必须用 `task_return` 结束。direct output recovery 没有独立的一次性尝试上限；只要仍有下一轮模型调用预算就继续纠偏。每次尝试都会写一条 `drift_recovery_attempted` 事件，便于宿主 UI 给用户显示"系统正在纠正…"提示：

```json
{
  "attempt": 1,
  "maxAttempts": 79,
  "maxRounds": 80,
  "limitReason": "max_rounds",
  "round": 9,
  "committedCount": 1,
  "reasonCode": "model.tool_call_required"
}
```

`maxAttempts` 是兼容字段，表示在当前 `maxRounds` 下 direct-output recovery 的理论上限；实际停止条件由 `maxRounds` / cancel 决定，而不是另一套隐藏 retry budget。

- 恢复成功 → run 继续，不会发 `run_rollback_targets`，也不会写 `run_failed`
- 恢复失败（模型再次返回 0 tool_calls 且已没有下一轮预算）→ 若没有成功 chat commit，写 `run_failed`（`userRetryable=true`）；若已有成功 chat commit，写 `run_partial_success` 并保留输出
- 上述 rollback / partial-success 语义与前端入口无关；普通发送、`/trigger`、regenerate 与 overswipe 只要进入 Agent run，都必须遵守同一 journal 与 host commit 契约。

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

Workspace event 不应内联大文件全文。面向 Agent/UI 的 payload 应记录 path、sha256、chars、words、patch ref；字节数仅保留在文件系统、checkpoint、hash 等内部实现边界。

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
direct_output_captured
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
  "textChars": 26,
  "textWords": 5,
  "hasAssistantText": true,
  "assistantTextChars": 26,
  "assistantTextWords": 5,
  "narration": {
    "source": "assistantText",
    "text": "I will write the artifact.",
    "totalChars": 26,
    "totalWords": 5,
    "truncated": false
  },
  "hasReasoning": true,
  "reasoningChars": 30,
  "reasoningWords": 6
}
```

`narration` 是可选字段，仅在模型回合包含工具调用且存在可展示 assistant visible text 时写入；它是模型轮次的展示投影，不是新的 run status，也不从 reasoning / thinking / thought 提取。事件中只保存短 preview；完整文本仍通过 `readModelTurn({ runId, round })` 的白名单 DTO 读取。

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

Checkpoint event must include checkpoint id、reason、file count and internal storage metadata. UI-facing timeline payloads should expose text metrics as chars/words, not raw byte counts.

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
chat_commit_started
chat_commit_requested
chat_commit_completed
chat_commit_failed
committed_message_rollback_completed
```

Commit event 必须包含 chat ref、checkpoint id、artifact path，并在 host bridge 已确认时包含 message id。`chat_commit_completed.messageId` 是 host bridge 在提交当时返回的聊天消息 id；当前 run 还会从数字型 message id 派生零基 `messageIndex`。这是提交时快照，不是对当前聊天消息位置的反向查询。

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

当前 `workspace_file_written` 同时覆盖 `workspace.write_file` 的 `replace` 与 `append` 成功结果，payload 必须携带 `mode`，便于 timeline 与恢复逻辑区分完整替换和原样追加。

如果副作用前需要保证恢复后不会重复执行，可以先写 pending event，再由恢复逻辑检查 pending 状态。这一点对 MCP/外部副作用尤其重要。

## 6. 实时订阅

前端订阅 API：

```js
const unsubscribe = await window.__TAURITAVERN__.api.agent.subscribe(runId, event => {});
```

要求：

- subscribe 不复播全部历史，除非 options 指定。
- UI 首次进入 run 页面可以读取最新页 `readEvents(runId, { beforeSeq, limit })`，再通过 subscribe / `afterSeq` 追新；需要回看更早轮次时继续用 `beforeSeq` 分页补拉。
- unsubscribe 必须幂等。
- 事件丢失时，UI 可通过 `afterSeq` 补拉。

## 7. 分页读取

Journal 读取需要支持：

```text
readEvents(runId, { afterSeq, limit })
readEvents(runId, { beforeSeq, limit })
```

移动端 timeline 不应该一次读取巨大 journal。
默认 timeline UI 应把 raw journal 投影成用户可见操作流，并使用窗口化渲染；状态机、provider state、checkpoint 等审计事件仍保留在 journal 中，但不应因为 UI 容量限制挤掉早期操作轮次。

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
direct_output_captured
tool_call_requested
tool_call_started
tool_result_stored
tool_call_completed
tool_call_failed
workspace_file_written
workspace_patch_applied
checkpoint_created
agent_loop_finished
artifact_assembled
commit_started
commit_draft_created
persistent_changes_committed
persistent_state_metadata_update_requested
persistent_state_metadata_updated
persistent_state_metadata_update_failed
run_committed
run_completed
run_partial_success
run_cancel_requested
run_cancelled
run_failed
```

这套事件已经足够支撑当前 tool loop、timeline、cancel、debug、commit、partial-success 和 diff 基础。`run_rollback_targets` 作为显式丢弃 / 旧版本清理事件形状保留，但不再是 committed drift 的默认终态路径。
