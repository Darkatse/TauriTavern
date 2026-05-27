# TauriTavern SubAgent Runtime

本文档记录当前 return-mode SubAgent 的实现基线、核心契约、Agent-friendly 设计原则与代码定位。后续开发多 Agent、handoff、真正后台并发前，应先读本文。

当前状态截至 2026-05-28：已实现 `agent.list`、`agent.delegate`、`agent.await` 与 return-mode child invocation 的 `task.return`。`agent.handoff` 仍是后续计划，当前没有模型可见 `agent.handoff` 工具。

## 1. 设计目标

SubAgent 不是 provider switch，也不是另开一个顶层 AgentRun。它是在同一个 AgentRun 内创建新的 AgentInvocation：

```text
AgentRun
  └─ root AgentInvocation
       ├─ AgentTask(return_to_parent) -> child AgentInvocation
       └─ AgentTask(return_to_parent) -> child AgentInvocation
```

这样可以保持：

- 同一个 run workspace、journal、cancel、checkpoint 与最终 commit 边界。
- 每个 Agent Profile 有独立 prompt、model binding、工具面与预算。
- return-mode child Agent 只产出任务结果，不直接写聊天消息。
- 主 Agent 负责整合、审稿与最终 `workspace.commit` / `workspace.finish`。

对 TauriTavern 来说，SubAgent 的价值不是让多个模型抢写同一个回复，而是让主写作 Agent 低摩擦地请求设定检查、剧情漏洞审阅、风格建议、候选片段、摘要与局部研究。

## 2. 当前流程

当前流程是按需执行，不是真正后台并发：

```text
root Agent calls agent.delegate
  ↓
runtime validates profile delegation policy
  ↓
create AgentTaskRecord + child AgentInvocation
  ↓
task status = queued
  ↓
root Agent calls agent.await
  ↓
runtime runs selected queued child invocation to terminal status
  ↓
child Agent calls task.return
  ↓
runtime writes result capsule and task summary
  ↓
agent.await returns markdown result to root Agent
```

`agent.delegate` 只注册任务，不立即 spawn 独立后台 worker。`agent.await` 会在当前 tool call 内驱动 queued child task。`nextCompleted` 模式会运行一个 queued task 并返回首个 terminal result；`allCompleted` 会依次运行 selected queued tasks。这个模型简洁、可测试、对现有 provider continuation 友好，但还不是 full background scheduler。

真正后台并发需要额外引入：

- run-scoped task scheduler / worker handle。
- child invocation 的独立 cancel / timeout / lifecycle 管理。
- 多 child 同时使用 model gateway 时的资源与 endpoint policy。
- UI timeline 中 queued/running/completed 的实时订阅更新。
- `agent.await` 从“驱动执行”变成“等待已经在后台执行的任务”。

## 3. 核心模型

当前 domain model 位于 `src-tauri/src/domain/models/agent/mod.rs`：

```rust
AgentInvocation {
    id,
    run_id,
    parent_invocation_id,
    profile_id,
    kind: Root | Subagent | Handoff,
    status: Created | Running | Completed | Failed | Cancelled | Transferred,
    exit_policy: RunFinishAllowed | TaskReturnRequired,
}

AgentTaskRecord {
    id,
    run_id,
    parent_invocation_id,
    child_invocation_id,
    target_profile_id,
    workspace_key,
    continuation: ReturnToParent | TransferControl,
    status: Queued | Running | Completed | Failed | Cancelled,
    task,
    budget,
    result_ref,
    error,
}
```

当前只创建 `continuation = ReturnToParent` 的 task。`TransferControl`、`Handoff`、`Transferred` 是为后续 handoff 保留的统一 delegation edge，不代表 handoff 已落地。

Invocation 与 task 文件由 `AgentInvocationRepository` 管理，当前文件实现位于：

```text
src-tauri/src/domain/repositories/agent_invocation_repository.rs
src-tauri/src/infrastructure/repositories/file_agent_repository/invocation_store.rs
```

物理存储位于 run workspace：

```text
invocations/<invocation-id>.json
tasks/<task-id>.json
agent-results/<child-invocation-id>.json
summaries/agents/<workspace-key>/result.md
scratch/agents/<workspace-key>/
```

`workspace_key` 面向 Agent 友好化：优先使用 target Agent id；同一个 run 中重复调用同一 Agent 时追加 `-002`、`-003`。

## 4. Tool Surface

当前模型可见工具位于 `src-tauri/src/application/services/agent_tools/agent/specs.rs`：

| Canonical | Model alias | 可见范围 | 语义 |
| --- | --- | --- | --- |
| `agent.list` | `agent_list` | 允许 delegation 的 root/active invocation | 列出当前 Agent 可调用的 Agent 目录 |
| `agent.delegate` | `agent_delegate` | `delegation.canDelegate = true` | 创建 return-mode 子任务 |
| `agent.await` | `agent_await` | `delegation.canDelegate = true` | 查询或收集自己创建的子任务结果 |
| `task.return` | `task_return` | runtime 只注入 return-mode child invocation | 提交 delegated task 结果并结束 child work |

不要把 `task.return` 写入 Profile `tools.allow`。它是 runtime-only 工具，由 `visible_tool_specs_for_invocation(..., TaskReturnRequired)` 注入。

`agent.delegate` 当前只接受：

```json
{
  "agentId": "scene-critic",
  "task": {
    "title": "检查剧情漏洞",
    "objective": "找出当前草稿里会破坏角色动机连续性的地方。",
    "context": {},
    "expectedOutput": {}
  },
  "budget": {
    "maxRounds": 4,
    "maxToolCalls": 12
  }
}
```

没有 `execution`、`continuation` 或 `invocationId` 参数。工具名已经表达了 continuation：`agent.delegate` 永远是 return-to-parent。

## 5. Child Invocation Policy

return-mode child Agent 必须遵守更窄的执行契约：

- `run.presentation = background`。
- 移除 `workspace.commit`、`workspace.finish`。
- 移除 `agent.list`、`agent.delegate`、`agent.await`。
- 注入 `task.return`。
- `exit_policy = TaskReturnRequired`。
- 可使用 target Agent Profile 的 model binding 与工具预算；delegate call 可进一步收窄 `maxRounds` / `maxToolCalls`。

实现入口：

```text
src-tauri/src/application/services/agent_runtime_service/delegation/policy.rs
src-tauri/src/application/services/agent_runtime_service/delegation/child_runtime.rs
src-tauri/src/application/services/agent_runtime_service.rs
```

子 Agent 如果调用 `workspace.finish`，runtime 会返回 recoverable tool error；如果在最大轮数内没有调用 `task.return`，child invocation 失败并把 task 标记为 failed。

## 6. Prompt 与 Result

child invocation 当前使用同一个 run 的 `input/prompt_snapshot.json` 作为 provider payload 基底，然后：

1. 解析 target Agent Profile。
2. 应用 child policy。
3. 调用 `resolve_model_binding()` 覆盖 target profile 的模型连接。
4. 用 target profile 的 materialized Agent system prompt 替换 messages。
5. 用 markdown task prompt 作为 user message。
6. 生成 child invocation 自己的 provider_state session id：`runId:invocationId`。

当前 child prompt assembly 是后端 runtime MVP，不是完整的前端 PromptAssemblyBroker handshake。也就是说，运行中 subagent 尚未完整复用 target profile 的独立 preset 组装流程；这是后续 invocation-scoped prompt assembly 的工作。

task prompt 渲染在：

```text
src-tauri/src/application/services/agent_runtime_service/delegation/rendering.rs
```

渲染原则：

- 面向子 Agent，而不是面向 runtime。
- 使用 markdown 标题组织 `Title`、`Objective`、`Context`、`Expected Output`。
- 不把 `taskId`、`invocationId`、`profileId`、`inside TauriTavern` 等运行时细节塞给模型。
- 明确提示可写 `summaries/`、`scratch/`，可读 `summaries/parent/`、`summaries/agents/`。

`task.return` 会写两份结果：

```text
agent-results/<child-invocation-id>.json      # runtime/audit structured result
summaries/agents/<workspace-key>/result.md    # parent/other Agents 可读 summary
```

`agent.await` 读取 structured result，但返回给父 Agent 的内容经过 markdown 渲染，只暴露 summary、findings、warnings、suggestedNextActions、questionsForCaller、artifacts、confidence 等 Agent 有用信息。

## 7. Invocation-scoped Workspace View

return-mode child Agent 不直接看到物理路径。它看到的是 invocation-scoped virtual workspace：

| 子 Agent 看到 | 物理路径 | 权限 | 含义 |
| --- | --- | --- | --- |
| `summaries/` | `summaries/agents/<workspace-key>/` | read/write | 当前任务的持久 notes |
| `scratch/` | `scratch/agents/<workspace-key>/` | read/write | 当前任务的临时 notes |
| `summaries/parent/` | `summaries/` 中排除 `agents/` 的父级私有摘要树 | read-only | 请求者提供或留下的 notes |
| `summaries/agents/` | `summaries/agents/` 中排除当前 child 自己 | read-only | 其他 delegated Agents 的 notes |

实现位置：

```text
src-tauri/src/application/services/agent_runtime_service/delegation/workspace_view.rs
```

关键规则：

- child 写入只能在 `summaries/` 或 `scratch/` 的具体文件下。
- `summaries/parent/` 和 `summaries/agents/` 只读。
- `summaries/parent/agents/...` 被拒绝，因为它把 parent private tree 和 sibling agent tree 混在一起。
- `summaries/agents/<self>/...` 被拒绝，当前 child 应使用 `summaries/...` 访问自己的 notes。
- NotFound / path-is-directory / denied alias 等错误会尽量转成模型输入的 virtual path，不泄露 `summaries/agents/<workspace-key>/...` 这种物理路径。

这个视图是 Agent-friendly 的核心之一：小模型不应该背 invocation id 或长路径，也不应该理解 runtime 存储布局。

## 8. Agent-friendly 原则

多 Agent 框架是为 Agent 服务的。当前实现遵守以下原则：

1. 模型可见工具只表达意图，不暴露 runtime 管理字段。
2. tool description 以调用者或执行者视角书写，不使用 parent/child/invocation 等错位语言。
3. 子任务 prompt 是 markdown briefing，不是裸 JSON task packet。
4. 子任务返回给父 Agent 的内容是 markdown result capsule，不是 runtime JSON dump。
5. 物理路径和模型路径分离，错误信息尽量回到模型输入路径。
6. child invocation 没有 chat commit 权限，避免多个 Agent 同时污染最终聊天消息。
7. 一个 invocation 内 provider-facing tool surface 稳定，不在 loop 中途动态增删工具。
8. 能 recover 的模型错误作为 tool error 返回；repository、journal、provider metadata、serialization 等宿主契约错误 fail-fast。

如果后续新增字段或工具，先问两个问题：

- 这个字段是 Agent 完成任务必须知道的吗？
- 它是从当前 Agent 的视角命名的吗？

如果答案是否定的，不要把它放进 model-facing prompt/tool/result。

## 9. 代码定位

SubAgent 主干入口：

```text
src-tauri/src/application/services/agent_runtime_service/delegation.rs
src-tauri/src/application/services/agent_runtime_service/delegation/list_tool.rs
src-tauri/src/application/services/agent_runtime_service/delegation/delegate_tool.rs
src-tauri/src/application/services/agent_runtime_service/delegation/await_tool.rs
src-tauri/src/application/services/agent_runtime_service/delegation/task_return_tool.rs
src-tauri/src/application/services/agent_runtime_service/delegation/child_runtime.rs
src-tauri/src/application/services/agent_runtime_service/delegation/policy.rs
src-tauri/src/application/services/agent_runtime_service/delegation/rendering.rs
src-tauri/src/application/services/agent_runtime_service/delegation/workspace_view.rs
src-tauri/src/application/services/agent_runtime_service/invocation.rs
src-tauri/src/application/services/agent_runtime_service/tool_execution.rs
```

Tool registry / dispatcher：

```text
src-tauri/src/application/services/agent_tools/agent/specs.rs
src-tauri/src/application/services/agent_tools/registry.rs
src-tauri/src/application/services/agent_tools/dispatcher.rs
```

Profile / policy：

```text
src-tauri/src/domain/models/agent/profile.rs
src-tauri/src/application/services/agent_profile_service.rs
src-tauri/src/infrastructure/repositories/file_agent_profile_repository/mod.rs
```

Persistence：

```text
src-tauri/src/domain/repositories/agent_invocation_repository.rs
src-tauri/src/infrastructure/repositories/file_agent_repository/invocation_store.rs
```

Tests：

```text
src-tauri/src/application/services/agent_runtime_service/tests.rs
src-tauri/src/infrastructure/repositories/file_agent_repository/tests.rs
```

## 10. 验证入口

重点测试应覆盖：

- `agent.list` policy 过滤与 model-facing 内容。
- `agent.delegate` 创建 task / child invocation / semantic workspace key。
- `agent.await` 选择 task、驱动 queued child、渲染 result capsule。
- child invocation tool surface：无 commit/finish/delegate/await，有 task.return。
- child system prompt 与 tool descriptions 不泄露不必要 runtime 细节。
- child workspace view 的 read/write/list/path error 映射。
- task.return artifact path normalizing 与 result summary 写入。

## 11. 已知边界

当前不是最终多 Agent runtime：

- 没有 `agent.handoff`。
- 没有真正后台并发 worker；child task 在 `agent.await` 内按需执行。
- return-mode child 默认不能 nested delegation，即使 profile schema 已有 `allowNestedDelegation` 字段。
- child invocation 尚未完整接入 invocation-scoped frontend PromptAssemblyBroker / target preset assembly。
- 没有跨 child 的主动通信；只能通过 `summaries/agents/` 读取其他 child 的结果 notes。
- 没有独立的 task timeout 取消边界；取消仍沿当前 run cancel receiver 传播。

这些边界是刻意保守的。后续扩展应保持现有不变量：同一 run 边界、invocation 独立 provider_state、root/handoff 才能 commit、tool result 不写 chat、model-facing surface 以 Agent 视角设计。
