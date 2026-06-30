# TauriTavern SubAgent Runtime

本文档记录当前 return-mode SubAgent 的实现基线、核心契约、Agent-friendly 设计原则与代码定位。`agent.handoff` 使用同一套 `AgentTaskRecord + AgentInvocation` 地基，但属于 foreground 接力流程；handoff 细节以 `docs/Agent/Handoff.md` 为准。后续开发多 Agent、task cancel 与 invocation-scoped prompt assembly 前，应先读本文和 `Handoff.md`。

当前状态截至 2026-06-29：已实现 `agent.list`、`agent.delegate`、`agent.await`、return-mode child invocation 的 `task.return`，run-scoped `ActiveRunHandle` / `AgentTaskScheduler` 后台 worker 基线，`agent.handoff` foreground 接力，以及 child / handoff invocation 的 PromptAssemblyBroker handshake。模型可见 task cancel 工具仍是后续计划，当前没有模型可见 `agent.cancel_task`。

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

当前 return-mode SubAgent 已经由 run-scoped scheduler 在后台执行：

```text
root Agent calls agent.delegate
  ↓
runtime validates profile delegation policy
  ↓
create AgentTaskRecord + child AgentInvocation
  ↓
AgentTaskScheduler spawns child worker
  ↓
child invocation runs independently to task.return / failed / cancelled
  ↓
root Agent may continue other tool work
  ↓
after the next root tool turn, completed child results are injected into the next model turn
  ↓
root Agent may also call agent.await to wait for selected tasks before continuing
```

`agent.delegate` 会创建 task / child invocation，并把任务提交给当前 run 的 `AgentTaskScheduler`。worker 使用独立 child invocation、独立 provider session id 与 child-only tool surface，在同一个 AgentRun 的 workspace / journal / cancel / commit 边界内运行。

`agent.await` 不再驱动 queued task 执行。它只查询或等待已经由 scheduler 执行的 child task：`nextCompleted` 等待首个 terminal result，`allCompleted` 等待 selected tasks 全部 terminal，`statusOnly` 立即返回当前状态。

父 Agent 无论是否显式调用 `agent.await`，只要它创建的 child task 已经 terminal，runtime 都会在下一次父 Agent tool turn 之后，把尚未在本轮上下文中出现过的结果作为 synthetic user message 注入下一轮模型请求。这个交付状态不写入 task record；当前只在 parent loop 内用内存集合去避免重复注入。这样避免把“已交付给父 Agent”固化成长期状态，同时保持 provider continuation 顺序清晰。

`agent.handoff` 创建 `continuation = TransferControl` 的 task 与 `kind = Handoff` 的后继 invocation。它不提交给 scheduler，也不返回 summary 给调用方；当前 invocation 记录完 handoff tool result 后转为 `Transferred`，executor 在同一个 `AgentRun` 内继续运行目标 Agent。已经 commit 过的 root / handoff owner 仍允许 handoff，最终 owner 可继续修订、再次 commit 并 `workspace.finish`。

`workspace.finish` 当前允许在仍有 unfinished return-mode child task 时结束 root / handoff owner 的 run stage。finish 会默认取消当前 parent 拥有的 unfinished return-mode tasks；run 收尾也会取消 run 内剩余 unfinished child tasks。这样不会因为缺少模型可见 cancel 工具或某个子任务卡住而拖长生成。handoff task 不在这个默认取消范围内；它代表当前接力链上的后继 invocation。

## 3. 核心模型

当前 domain model 位于 `src-tauri/crates/tt-domain/src/models/agent/mod.rs`：

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
    result_ref,
    error,
}
```

`continuation = ReturnToParent` 的 task 由 scheduler 后台执行，并最终通过 `task.return` 把结果交回调用者。`continuation = TransferControl` 的 task 表示 handoff：它不进入后台 scheduler，而是在当前 foreground invocation 转为 `Transferred` 后，由 executor 串接启动 `Handoff` invocation。

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
summaries/<workspace-key>-result.md
```

`workspace_key` 面向 Agent 友好化：优先使用 target Agent id；同一个 run 中重复调用同一 Agent 时追加 `-002`、`-003`，并用于生成自动 task result summary 文件名。

## 4. Tool Surface

当前模型可见工具位于 `src-tauri/src/application/services/agent_tools/agent/specs.rs`：

| Canonical | Model alias | 可见范围 | 语义 |
| --- | --- | --- | --- |
| `agent.list` | `agent_list` | 允许 delegation 的 root/active invocation | 列出当前 Agent 可调用的 Agent 目录 |
| `agent.delegate` | `agent_delegate` | `delegation.canDelegate = true` | 创建 return-mode 子任务并提交后台 worker |
| `agent.handoff` | `agent_handoff` | `delegation.canHandoff = true` | 创建 TransferControl task，并让目标 Agent 接手同一 run 的下一阶段 |
| `agent.await` | `agent_await` | `delegation.canDelegate = true` | 查询或等待自己创建的子任务结果 |
| `task.return` | `task_return` | runtime 只注入 return-mode child invocation | 提交 delegated task 结果并结束 child work |

不要把 `task.return` 写入 Profile `tools.allow`。它是 runtime-only 工具，由 `visible_tool_specs_for_invocation(..., TaskReturnRequired)` 注入。

`agent.delegate` 当前只接受：

```json
{
  "agentId": "scene-critic",
  "task": {
    "objective": "找出当前草稿里会破坏角色动机连续性的地方。",
    "context": {},
    "expectedOutput": {}
  }
}
```

`task.title` 是可选展示名；只有 `task.objective` 承载必须完成的任务目标。

没有 `budget`、`execution`、`continuation` 或 `invocationId` 参数。工具名已经表达了 continuation：`agent.delegate` 永远是 return-to-parent，`agent.handoff` 永远是 transfer-control。return-mode SubAgent 的硬运行预算只来自 target Agent Profile 与宿主运行时策略；主 Agent 如需表达“简短”“快速”“只返回摘要”，应写入 task brief / expectedOutput，而不是 runtime policy。

## 5. Child Invocation Policy

return-mode child Agent 必须遵守更窄的执行契约：

- `run.presentation = background`。
- 前端“可作为子 Agent”会写入 `run.directRunnable = false`；直接启动入口会 fail-fast，return-mode child invocation 仍可通过 `agent.delegate` 运行。
- 移除 `workspace.commit`、`workspace.finish`。
- 移除 `agent.list`、`agent.delegate`、`agent.handoff`、`agent.await`。
- 注入 `task.return`。
- `exit_policy = TaskReturnRequired`。
- 使用 target Agent Profile 的 model binding 与工具预算；delegate call 不能覆盖或收窄 `maxRounds` / `maxCallsPerRun`。
- child 与请求它的 Agent 使用同一套逻辑 workspace path，不存在 return-mode 专用目录映射；可见/可写 root 仍由 target Agent Profile 的 `workspace.visibleRoots` / `workspace.writableRoots` 决定。

实现入口：

```text
src-tauri/src/application/services/agent_runtime_service/delegation/policy.rs
src-tauri/src/application/services/agent_runtime_service/delegation/child_runtime.rs
src-tauri/src/application/services/agent_runtime_service.rs
```

子 Agent 如果调用 `workspace.finish`，runtime 会返回 recoverable tool error；如果在最大轮数内没有调用 `task.return`，child invocation 失败并把 task 标记为 failed。

## 6. Prompt 与 Result

child invocation 先解析 target Agent Profile 并应用 child policy。随后根据 target Profile 的 preset binding 选择组装路径：

- `preset.mode = ref`：runtime 从 root `input/prompt_snapshot.json` 读取 `frozenRunInputSnapshot`，注册 pending broker request，并通过轻量 `prompt_assembly_requested` 事件通知前端。前端用 `read_agent_prompt_assembly_request` 按 `assemblyId` 读取完整 request，让 PromptAssemblyBroker 使用 target Profile 的 `preset.ref`、Agent system prompt、child task prompt 与 frozen input 重新组装 child prompt snapshot。前端完成后调用 `resolve_agent_prompt_assembly` 回填；runtime 校验 `contextPolicy`，把组装结果写入 `input/invocations/<childInvocationId>/prompt_snapshot.json`，并把 request metadata / result metadata 写入 `input/invocations/<childInvocationId>/prompt_assembly.json`，再进入 child tool loop。
- `preset.mode = currentPromptSnapshot` / `none`：保持兼容路径，使用同一个 run 的 `input/prompt_snapshot.json` 作为 provider payload 基底，并由后端替换为 target Profile 的 materialized Agent system prompt + markdown task prompt。

两条路径都会在进入模型前调用 `resolve_model_binding()` 覆盖 target profile 的模型连接，并生成 child invocation 自己的 provider_state session id：`runId:invocationId`。

child invocation 的 Skill 可见性同样按 invocation 解析：`skills.visible` / `skills.deny` / read budget 来自 target Profile；active scope 顺序为 `global -> preset -> profile -> character`。其中 `preset` 对 `preset.ref` Profile 使用 target Profile 自己的 preset，对 `currentPromptSnapshot` Profile 使用 root run 启动时固化的 ambient preset ref；`character` 使用 root run 固化的 ambient character ref。解析结果写入 `input/invocations/<childInvocationId>/resolved_skills.json`，并记录带 `invocationId` 的 `skill_scopes_resolved` event。

task prompt 渲染在：

```text
src-tauri/src/application/services/agent_runtime_service/delegation/rendering.rs
```

渲染原则：

- 面向子 Agent，而不是面向 runtime。
- 使用 markdown 标题组织 `Title`、`Objective`、`Context`、`Expected Output`。
- 不把 `taskId`、`invocationId`、`profileId`、`inside TauriTavern` 等运行时细节塞给模型。
- 明确提示子 Agent 使用任务 brief 中出现的普通 workspace path；如果需要写 notes/artifact，只写入 target Profile 授权的 writable root。
- 不解释物理映射、CAS 参数或 runtime id；路径语言始终从执行 Agent 能直接操作的 workspace 出发。

`task.return` 会写两份结果：

```text
agent-results/<child-invocation-id>.json      # runtime/audit structured result
summaries/<workspace-key>-result.md           # 普通 workspace summary，父 Agent 可直接读取
```

`agent.await` 与后台结果自动注入都读取 structured result，但返回给父 Agent 的内容经过 markdown 渲染，只暴露 summary、findings、warnings、suggestedNextActions、questionsForCaller、artifacts、confidence 等 Agent 有用信息。渲染 capsule 末尾会追加一个面向当前父 Agent 的轻量 continuation hint：提醒这些结果是上下文，不覆盖当前 Agent Profile / task；下一步仍应继续使用 Agent tools，并按当前 foreground/background 与 commit 状态通过 `workspace_commit` / `workspace_finish` 收口。这个提示属于父 Agent 语言界面，不写入 task result structured payload。

## 7. Shared Workspace Semantics

return-mode child Agent 看到的 path 与请求它的 Agent 看到的 path 是同一套逻辑 workspace path。runtime 不再为 child 创建 `summaries/parent/`、`summaries/agents/` 或 `scratch/agents/<workspace-key>/` 这类虚拟映射。

实现位置：

```text
src-tauri/src/application/services/agent_runtime_service/delegation/workspace_policy.rs
```

关键规则：

- child 使用 target Profile 的 `workspace.visibleRoots` / `workspace.writableRoots` 过滤同一个 run workspace；只改权限视图，不改路径。
- Agent-facing prompt / tool description 只提示 visible/writable roots 与任务 brief 中的普通 workspace path。
- `task.return` 自动把 markdown summary 写到 `summaries/<workspace-key>-result.md`；子 Agent 不需要也不应该手动写这个 result 文件。
- 如果 child 需要支持性 notes，应选择明确的普通 workspace path，例如调用方指定的 `summaries/scene-critic-notes.md`；重复调用同一 Agent 时，调用方或子 Agent 可用更具体文件名避免碰撞。
- `persist/` 仍只是本 run 的 projection；return-mode child 写入不会直接 promote，只有 root / handoff foreground owner 的 `workspace.finish` 收尾成功才会写回稳定 chat workspace。

这个语义更接近多人协作同一个项目目录：权限由 Profile 控制，路径不再承担 runtime 隔离概念。

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
9. `agent.await` 是需要结果或状态时的等待/查询工具，不是 delegation 后必须执行的收集步骤；调用者可以先继续其它工作。
10. `taskId` 只作为可选的 opaque task handle；常规情况下调用者可以不传 taskIds，让 `agent.await` 面向自己启动的任务集合。
11. 调用方给子任务时应传递相关 workspace path 与期望 artifact 形态；子 Agent 不需要猜 runtime 存储布局。
12. 子任务结果 capsule 应帮助父 Agent 低摩擦地回到当前工具流程；不要让返回文本看起来像普通用户消息或最终 answer。

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
src-tauri/src/application/services/agent_runtime_service/delegation/handoff_tool.rs
src-tauri/src/application/services/agent_runtime_service/delegation/await_tool.rs
src-tauri/src/application/services/agent_runtime_service/delegation/task_return_tool.rs
src-tauri/src/application/services/agent_runtime_service/delegation/child_runtime.rs
src-tauri/src/application/services/agent_runtime_service/delegation/policy.rs
src-tauri/src/application/services/agent_runtime_service/delegation/rendering.rs
src-tauri/src/application/services/agent_runtime_service/delegation/workspace_policy.rs
src-tauri/src/application/services/agent_runtime_service/scheduler.rs
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
src-tauri/crates/tt-domain/src/models/agent/profile.rs
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
- `agent.delegate` 创建 task / child invocation / semantic workspace key，并提交 scheduler。
- scheduler 后台运行 child worker，完成后写 terminal task / invocation 状态。
- `agent.await` 选择 task、等待或查询 scheduler 结果、渲染 result capsule。
- 父 Agent 下一次 tool turn 后自动收到尚未出现过的 terminal child results。
- `workspace.finish` 会取消 unfinished return-mode child tasks，而不会被其阻塞。
- handoff 创建 TransferControl task，不进入 scheduler；当前 invocation 转为 Transferred 后，同一 run 串接执行目标 handoff invocation。详细流程见 `Handoff.md`。
- child invocation tool surface：无 commit/finish/delegate/handoff/await，有 task.return。
- child system prompt 与 tool descriptions 不泄露不必要 runtime 细节。
- child workspace policy 只调整 root 可见/可写权限，不映射路径。
- task.return artifact path normalizing 与 result summary 写入。

## 11. 已知边界

当前不是最终多 Agent runtime：

- 没有模型可见 `agent.cancel_task`；当前只有 run cancel 与 finish 默认取消 unfinished child tasks。
- return-mode child 默认不能 nested delegation，即使 profile schema 已有 `allowNestedDelegation` 字段。
- return-mode child 不能 handoff；handoff 只属于 root / active foreground owner。handoff 的字段、事件、prompt brief 与扩展边界见 `Handoff.md`。
- 没有跨 child 的主动通信；多个 child 只能通过同一个 workspace 中的普通文件和 `task.return` 结果协作。
- 没有独立的 task timeout 取消边界；child worker 使用当前 scheduler / run cancellation path。

这些边界是刻意保守的。后续扩展应保持现有不变量：同一 run 边界、invocation 独立 provider_state、root/handoff 才能 commit、tool result 不写 chat、model-facing surface 以 Agent 视角设计。
