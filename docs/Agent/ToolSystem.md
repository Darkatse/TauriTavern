# TauriTavern Agent Tool System

本文档定义 Agent Tool System 的 Catalog、Snapshot、Request Gate、执行结果、审批与前端/扩展/MCP 边界。

Agent 的能力上限很大程度由工具决定，但第一期更重要的是工具架构正确，而不是工具数量多。

## 1. 目标

工具系统必须做到：

- 工具可发现。
- 工具可按 profile/plan/policy 控制可见性。
- 工具调用可审计。
- 工具结果能进入 ContextFrame。
- 工具错误能被模型和用户理解。
- 工具副作用可 checkpoint/rollback 或明确不可回滚。
- MCP、extension 与 builtin 工具使用同一控制面契约，并保留各自 executor。

## 2. 非目标

第一期不做：

- shell 工具。
- 任意后端 JS 执行。
- 世界书动态脚本作为后端工具。
- 任意远端工具自动注册。
- MCP Sampling 自动模型调用。
- 大而全的插件市场。

## 3. Canonical 模型与事实所有权

工具系统没有万能 Spec。每类事实只属于一个模型：

| 模型 | 拥有的事实 | 不应拥有 |
| --- | --- | --- |
| `ToolId` | 跨来源稳定身份：provider + native name | model alias、显示名 |
| `ToolDescriptor` | title、description、input/output schema、raw annotations | Profile 权限、预算、executor、model alias |
| `ToolCatalog` | 当前来源贡献的基础 descriptor 集合 | invocation 状态 |
| `ToolBinding` / `InvocationToolSnapshot` | invocation 冻结后的 descriptor、alias、预算 | 当前 registry 查询 |
| `ToolTurnContract` | 当前模型调用可请求的 ToolId 与 typed choice | Provider JSON |
| `AgentModelTool` | 当前 request 的 ToolId、alias、description、input schema | UI 字段、执行 handler |
| `AgentToolCatalogItemDto` | Profile UI 所需的展示与 schema 字段 | model alias、执行权限 |

维护规则：新增字段先判断它由哪个边界拥有；不要重新引入同时服务 Catalog、Provider、UI 和执行器的复合类型。

## 4. AgentToolResult

当前 Agent 执行结果使用：

```rust
AgentToolResult {
    call_id,
    tool_id,
    content,
    structured,
    is_error,
    error_code,
    resource_refs,
}
```

`call_id + tool_id` 必须与原始 `ToolInvocation` 完全一致。结果进入 effect 解释、审计存储或 provider transcript 前都要验证该身份。

`structured` 是 runtime、完整 JSON audit 与 Timeline UI 的内部数据，不进入模型上下文。所有 provider 对模型统一编码为一个稳定 JSON 字符串：`{ ok, content, errorCode, resourceRefs }`。模型需要采取行动的信息必须出现在简洁的 `content` 中；内部版本哈希、统计字段与重复正文不得占用模型注意力。

原则：

- 大结果写 resource ref，不内联到 journal。
- `is_error = true` 可以是模型可恢复错误，不一定让 run Failed。
- 系统级错误，如 journal append failed、序列化失败或身份不一致，必须让 run Failed。
- 多模态 content block、usage 和 provenance 等到出现真实消费者时再扩展，不预建通用 `ToolOutput`。

## 5. Tool Call 生命周期

```text
resolved Profile + invocation exit policy
  ↓
compile immutable InvocationToolSnapshot
  ↓
compile ToolTurnContract + request-scoped AgentModelTool projection
  ↓
model emits snapshot-bound alias
  ↓
decode alias through this turn only into canonical ToolInvocation
  ↓
ToolRequestGate checks snapshot/turn/choice and reserves frozen budget
  ↓
adapt to Agent runtime control, builtin dispatcher, or McpService
  ↓
write result
  ↓
append journal
  ↓
next AgentModelRequest includes the model-safe result projection
```

Journal：

```text
tool_call_requested
tool_call_started
tool_call_completed / tool_call_failed
```

现有 Agent 工具已收敛到 invocation-local `ToolRequestGate`：所有模型调用先以 canonical `ToolId` 检查 snapshot、turn membership、typed choice 与预算，再进入 Agent runtime control handler、builtin dispatcher 或 `McpService`。MCP 是第二个 executor，但只需要在唯一执行入口按 provider 分支；没有引入通用 executor trait/router。当前 Ask 与 Allow 同样自动执行，不建立 approval port、pending-call 状态或 UI。执行过程中不得重新读取 Profile 或 global registry 来重算工具可见性；MCP 仅在发送前重新读取 registration 的 Active/permission 事实。

## 6. Policy Resolution

输入：

- user global policy
- profile tool policy
- plan node tool policy
- tool source policy
- platform policy
- runtime budget

当前 Agent snapshot compiler 的输入是 resolved Profile、`AgentInvocationExitPolicy`、只读 builtin catalog 与从 `McpService` cached catalog 解析出的 MCP descriptors，输出有序 bindings：冻结后的 descriptor、snapshot 内唯一 model alias、per-tool 调用上限，以及 invocation 总调用上限。规则是 Profile `deny` 优先于 `allow`；return-mode child 再按宿主拥有的 exit policy 移除 commit、finish 与 delegation control tools，并注入唯一的 `task.return`。Profile allow 顺序就是 provider advertisement 顺序。

后续接入 plan 与统一 approval 时，policy resolver 的完整输出为：

```text
visible: bool
callable: bool
approvalRequired: bool
reason
budget
```

规则：

- user deny 最高。
- platform deny 不可覆盖。
- plan node deny/allow 优先于 profile allow。
- deny 优先 allow。
- approval 不是 deny。
- 未允许工具默认不可见。

模型返回的名称只能是当前 turn 广告的 alias；Gateway 在本次 request 内把 alias 精确解析为 `ToolInvocation { call_id, tool_id, arguments, provider_metadata }`。未知、canonical-name 直呼或其它 snapshot 的 alias 都作为 provider contract violation fail-fast，不再通过全局 registry 猜测。预算耗尽仍是模型可恢复的 tool result。

## 7. 内置工具

### 7.0 当前实现

截至 2026-08-09，当前 registry 开放 agent / chat / world info / dice / skill / workspace 六类内建工具；MCP descriptors 由 `McpService` 在 invocation preparation 时按 Profile 选择提供。每个 builtin 直接声明 canonical `ToolDescriptor`，Registry 只从这些声明构造 `ToolCatalog`；canonical ID 统一为 `builtin:<native-name>`。Catalog 只保存中性描述，不承载 model alias、Profile permission、executor 或 policy facts；重复 ID fail-fast。`tools.list()` 使用专用 UI DTO 合并 builtin 与 cached MCP 投影，model alias 仍只存在于 invocation binding。

每个 root、return-mode child 与 handoff invocation 启动时只编译一次 `InvocationToolSnapshot`。Compiler 按 Profile allow 顺序从 Catalog 复制 descriptor，应用 Profile workspace/description facts、deny 与调用上限，再冻结 alias；当前 `ToolTurnContract` 只通过 `all(...)` 使用 snapshot 全集，生产 compiler 固定选择 `ToolChoice::Auto` 并生成 request-scoped `AgentModelTool`。child/handoff 提示词、provider request、continuation hint、alias decode 与 gate 均消费这份 snapshot/turn，不再各自过滤 Profile。root PromptManager 仍在后端 invocation 创建前通过兼容桥组装，其 Agent prompt preview 使用同一个 compiler；真正 provider advertisement 与执行权限以随后持久化的 root snapshot 为准。return-mode child 由 exit policy 移除 chat commit / run finish / delegation tools，并注入 runtime-only `task.return`；handoff invocation 使用目标 Profile 编译新的 snapshot。child 与请求它的 Agent 使用同一套逻辑 workspace path；runtime 只按 target Profile workspace policy 调整当前 invocation 的 visible/writable roots，不做 child 专用路径映射。

Agent-facing 文案必须从调用或执行 Agent 的角度描述可操作路径：`agent.delegate` 鼓励在 task brief 中给出相关 workspace path 与期望 artifact；`agent.handoff` 鼓励给出 objective、workspace refs、context、constraints 与 completion criteria；return-mode workspace tools 只提示 visible/writable roots 与任务中的普通 workspace path，不暴露 physical mapping、CAS 参数或 runtime id。

| Canonical name | Model-facing alias | Side effect | 状态 |
| --- | --- | --- | --- |
| `agent.list` | `agent_list` | 只读列出当前 Agent 可调用的 Agent Profile 目录 | 已落地 |
| `agent.delegate` | `agent_delegate` | 创建 return-mode 子任务，不立即返回结果 | 已落地 |
| `agent.handoff` | `agent_handoff` | 创建 TransferControl task 与 Handoff invocation，让目标 Agent 接手同一 run 的下一阶段 | 已落地 |
| `agent.await` | `agent_await` | 查询或等待当前 invocation 创建的 delegated task 结果 | 已落地 |
| `task.return` | `task_return` | return-mode child invocation 提交任务结果并结束 child work；runtime-only，不允许写入 profile tools.allow | 已落地 |
| `chat.search` | `chat_search` | 只读搜索当前聊天，返回 message index、snippet 与 ref | 已落地 |
| `chat.read_messages` | `chat_read_messages` | 只读按 message index 读取当前聊天消息或行范围 | 已落地 |
| `worldinfo.read_activated` | `worldinfo_read_activated` | 只读读取本轮 run 捕获的最终激活世界书条目 | 已落地 |
| `dice.roll` | `dice_roll` | 只读随机投骰；支持 `d6`、`1d20`、`3d6+4` 等轻量 dice notation，默认 Profile 不启用 | 已落地 |
| `skill.list` | `skill_list` | 只读列出已安装 Skill 索引摘要 | 已落地 |
| `skill.search` | `skill_search` | 只读搜索单个可见 Skill 内的 UTF-8 文本文件，返回 snippet 与 ref | 已落地 |
| `skill.read` | `skill_read` | 只读读取已安装 Skill 内的 UTF-8 文本文件或范围 | 已落地 |
| `workspace.list_files` | `workspace_list_files` | 只读列出模型可见 workspace 文件 | 已落地 |
| `workspace.search_files` | `workspace_search_files` | 只读搜索模型可见 workspace UTF-8 文本文件，返回 snippet 与 ref | 已落地 |
| `workspace.read_file` | `workspace_read_file` | 只读读取 UTF-8 文本或范围，完整读取记录 read-state | 已落地 |
| `workspace.write_file` | `workspace_write_file` | 写 run workspace 文件，成功后 checkpoint | 已落地 |
| `workspace.apply_patch` | `workspace_apply_patch` | 单文件精确替换，成功后 checkpoint | 已落地 |
| `workspace.commit` | `workspace_commit` | 将 workspace 文件提交到当前 chat message | 已落地 |
| `workspace.finish` | `workspace_finish` | 结束工具循环，进入 run 收尾 | 已落地 |

MCP binding 使用 `mcp/<registration-uuid>:<native-name>` canonical ID。模型 alias 面向 Agent 可读性生成：`mcp__<normalized server displayName>__<normalized nativeName>`，例如 `issue.create` 在 `my server` 下成为 `mcp__my_server__issue_create`；碰撞以 `__2`、`__3` 确定性消歧，64-byte 截断优先保留工具动作部分。decoder 只查当前 snapshot 的 alias→ToolId map，不从 alias 反推 registration/native name。

Agent 未选择 MCP ToolId 时不访问 MCP 存储；否则只读取 application memory→disk catalog，不会隐式联网 discovery。MCP tool 无缓存、registration 存储损坏、Paused、Off、目录消失或 root input schema 不是显式 object 时，只省略该工具，并在 Profile 配置警告与 invocation 时间线显示 diagnostic。Ask/Allow 当前都直接调用；发送前 `McpService` 重新检查 registration、Active、permission、object arguments、256 KiB 上限与 cancellation。

默认模型可见/可写 workspace 前缀为：

```text
output/
scratch/
plan/
summaries/
persist/
```

runtime 额外提供 `tool-results/`：对所有 invocation 可见、永远不可写且从不 commit。它不进入 Profile 可配置 root universe。

这些前缀由 resolved Profile 写入 run manifest，Profile 只能收窄 root universe。`persist/` 是 chat workspace 级持久 root 的 run projection：模型在 run 中通过普通 workspace 工具写入，`workspace.finish` 收尾成功后才 promote 回稳定 chat workspace；失败或取消的 run 不会写回。

工具参数会以 create-only 语义写入 `tool-args/call_<sha256_8byte_hex(call-id)>.json`，工具结果会以相同语义写入 `tool-results/call_<sha256_8byte_hex(call-id)>.json`；重复 `call_id` 或 digest 路径冲突在覆盖既有审计事实前 fail-fast。本地文件名只使用 SHA-256 前 8 字节 hex。provider 返回的原始 `call_id` 只作为不透明业务 ID 保存在 JSON 内容、journal payload 与下一轮 `AgentModelContentPart::ToolResult` 中，不作为本地文件名。`AgentToolResult` 同时保存 canonical `ToolId`；dispatcher outcome 在解释 `AgentToolEffect` 前必须匹配原始 `ToolInvocation`，最终结果落盘前再次验证。Gateway 只通过当前 request 的 `ToolId -> model alias` 投影编码历史结果。工具结果不会写入 SillyTavern chat 楼层。

MCP 结果总是先完整写入 JSON audit，再决定模型投影。MCP 的 structured content、diagnostics 与可操作错误详情会在该投影中转为带标签的 `content` 段落；内部 `structured` 仍只供审计。模型可见 `content` 超过当前 Profile 的 `tools.mcpResultInlineCharLimit`（默认 50,000）时，runtime 额外写入同一完整 `content` 的 `.txt` 可读视图；超长物理行只在该视图中换行，精确原始结果仍由 JSON audit 保存。模型收到原始 `content` 最多前 3,000 个 Unicode 字符的前缀预览、可读视图与 audit 路径、字符数、resource refs，以及当前 snapshot 中 `workspace.read_file` / `workspace.search_files` 的真实 alias 和分段读取指引。该正整数随 resolved Profile 固定到 invocation，root、SubAgent 与 handoff 各自使用其 Profile 值。字符统计复用 domain `TextMetrics`，不依赖具体模型 tokenizer。若 Profile 没有读工具，路径仍作为用户可见 result reference 返回，不静默截断，也不为此临时扩大工具权限。

Timeline 的隐藏、side-effect 关联与 builtin 类型判定只使用 journal 中的 canonical `toolId`；native `name` 仅用于用户可读标题。Model Turn UI 投影保留 `toolId`，同 native name 的外部工具以 canonical identity 区分。

完整 snapshot 以 create-only 语义写入 `input/invocations/<invocation-id>/tool_snapshot.json`，同 ID 重复写入 fail-fast；`context_assembled` 记录该路径、紧凑 snapshot 摘要与完整 `ToolTurnContract`，每次 `model_request_created` 再记录本轮 contract。snapshot 文件保存 schema version、冻结 descriptor、alias 与预算，active invocation 不依赖后续 Profile/registry 查询。当前 snapshot ID 在单个 run 内等于 invocation ID；run journal 和 workspace 路径共同构成审计作用域。

Profile 的 `maxCallsPerRun` 是历史字段名，实际语义一直是 invocation 生命周期。Snapshot 使用真实名称 `maxCallsPerInvocation`；invocation-local `ToolRequestGate` 按 canonical `ToolId` 检查并预留总预算和 per-tool 预算。Gate 在当前串行 tool loop 中通过唯一 `&mut` 所有权保证一次“检查并预留”不可交错；预算从承载 workspace/skill 状态的 `AgentToolSession` 中移除。Profile 字段的纯命名迁移留到 selector schema 迁移时一次完成。

`workspace.apply_patch` 使用 Claude Code 风格的 `old_string` / `new_string` 单文件精确替换。`old_string` 必须来自模型本 run 已读到的文本片段，或来自本 run 创建/完整替换后已经完整已知的文件；runtime 仍会读取当前完整文件检查版本与全文件唯一匹配，但不会把完整文件隐式塞回模型上下文。版本变化、匹配 0 次或多次会作为 recoverable tool error 返回模型；基于部分读取的 patch 一旦失败，同文件后续 patch 必须先完整读取，避免模型在不确定上下文上反复试错。`replace_all=true` 可能修改未读位置，因此必须在完整读取后使用。`workspace.write_file` 支持 `mode = replace | append`，默认 `replace`。`replace` 对已存在文件复用同一个 session read-state 做 CAS：模型不需要传 `expectedSha256`，schema 不暴露 overwrite policy；若文件在最近读取/写入后被其他 invocation 修改，会返回可恢复的 stale-file 工具错误，要求重新读取后再写。`append` 会把 `content` 原样追加到文件末尾，目标缺失时创建文件；不会自动补换行，模型需要新行时应把前导 `\n` 放进 `content`。`append` 工具调用本身只在新建文件或追加前文件已完整读入且版本匹配时更新完整 read-state，避免未读既有内容在同一轮内被隐式授权为后续 rewrite/patch 的依据。模型传入的非法 path、空 path、非法 mode、不可见/不可写 path 也作为可恢复工具错误回填；目标 path 实际指向目录的读写请求会作为 `workspace.path_is_directory` 业务错误回填，提示模型改用 `workspace_list_files`。repository 内部 escape/symlink/journal、checkpoint、序列化、取消和模型响应结构错误仍 fail-fast。

`workspace.commit` 与 `workspace.finish` 的契约：模型可以多次 commit；当全部修订与 commit 完成后，必须用 `workspace.finish` 收口，不能用纯文本代替最终 answer。foreground `workspace.finish` 要求同一 run 已经有至少一次成功 chat commit；在 handoff 链中，这个 commit 可以来自前一个 foreground owner。`workspace.commit` 工具的返回字符串只做温和提醒，提示模型可继续修订并再次 commit，但最终不要忘记 finish。

如果模型一回合内仍然返回 0 个 tool_calls（drift），loop runner 会做"软纠正"（issue #64）：把模型的纯文本回复捕获到 workspace 的 `direct_output.md`（默认 `output/direct_output.md`，实际跟随当前 profile 的 messageBody artifact root），写 `direct_output_captured` 与 checkpoint；再把该 assistant 回复推进 history，并追加一条 `user` 角色的合成提醒，让模型在下一轮通过当前 invocation 的结束工具补回流程。root run 使用 `workspace_commit` / `workspace_finish`，return-mode child 使用 `task_return`。direct output 本身没有独立的一次性上限；只要仍有下一轮模型调用预算，就会继续纠偏。每次都会写一条 `drift_recovery_attempted` 事件。

- 软纠正后模型调用了 `workspace_finish`，或继续修订并再次 `workspace_commit` 后再 `workspace_finish` → run 继续，无 rollback。
- 软纠正后模型再次 0 tool_calls 且已经没有下一轮预算 → 回落到 `model.tool_call_required` 失败路径；若没有成功 chat commit，则写 `run_failed`（`userRetryable=true`）；若已有成功 chat commit，则写 `run_partial_success`，保留已提交聊天输出并以 warning 暴露底层错误。

违反此契约的其它形态（`agent.tool_after_finish` / `agent.max_tool_rounds_exceeded`）目前不走软纠正，直接进入同一终态分类：没有成功 chat commit 时 fail-fast；已有成功 chat commit 时 `run_partial_success`。细节见 `RunEventJournal.md`。

当前没有 shell、extension bridge、profile routing、Plan Mode runtime、模型可见 task cancel 或审批工具；Legacy MCP 已通过独立的 JS generation seam 接入，不复用 Agent executor。

### 7.1 Agent Delegation Tools

`agent.list`

- Read-only。
- 根据当前 Profile delegation policy 与 target Profile callable policy，列出可调用 Agent。
- 支持 `purpose = any | delegate | handoff`、`query`、`limit`。

`agent.delegate`

- Mutating/control。
- 创建 return-mode `AgentTaskRecord` 与 child `AgentInvocation`。
- 参数只包含 `agentId` 与 `task`；不接受 `budget`、`execution`、`continuation`、`invocationId` 等 runtime 字段。
- SubAgent 的硬运行预算来自 target Agent Profile 与宿主运行时策略；调用方只能通过 task brief 表达任务范围和期望输出形态。
- 子任务提交给当前 run 的 `AgentTaskScheduler` 后台执行；父 Agent 不需要阻塞在 `agent.delegate` 上。

`agent.handoff`

- Mutating/control。
- 创建 transfer-control `AgentTaskRecord` 与 `Handoff` invocation；不进入后台 scheduler，也不返回 summary 给当前 Agent。
- 参数只包含 `agentId` 与 `handoff` brief；不接受 `execution`、`continuation`、`invocationId` 等 runtime 字段。
- 仅在当前 Profile `delegation.canHandoff = true` 且工具显式可见时可用；target Profile 必须 `callable = true`、`allowAsHandoffTarget = true`，并通过 `allowedCallers` 与 `maxHandoffDepth` 校验。
- 若当前 invocation 仍有未 terminal 的 return-mode delegated task，`agent.handoff` 返回可恢复工具错误；模型必须先 `agent.await`、等待任务结束，或通过当前可用路径收口。
- 一次 handoff 成功后，调用方 invocation 必须停止继续调用工具；executor 会在同一 run 内串接运行目标 Agent。
- 详细流程、结构体、event 序列与测试入口见 `docs/Agent/Handoff.md`。

`agent.await`

- Read-only/control。
- 查询或等待当前 invocation 自己创建的 delegated tasks。
- 不驱动 queued task 执行；后台 worker 完成后，`agent.await` 只负责等待/渲染已有状态。
- 返回 markdown result capsule，并保留 structured result 给 journal/audit。
- 即使父 Agent 不显式 await，terminal child results 也会在下一次父 Agent tool turn 后注入下一轮模型请求。
- `taskIds` 是可选的精确句柄；省略时面向当前调用者自己启动的 delegated tasks。

`task.return`

- return-mode child invocation 专用。
- Profile 不得显式允许该工具；runtime 根据 `TaskReturnRequired` exit policy 注入。
- 调用后写入 `agent-results/<child-invocation-id>.json` 与 `summaries/<workspace-key>-result.md`。
- child invocation 必须用它结束工作，不能用 `workspace.finish`。

### 7.2 后续内置工具候选

后续优先补齐策略与 checkpoint 工具：

```text
workspace.create_checkpoint
```

### 7.3 Workspace Tools

`chat.search`、`workspace.search_files` 与 `skill.search` 共享纯查询规范化规则：包含字母、数字或 `_` 的 query 继续使用既有分词评分；非空 query 若分词后没有任何词项，则保留 trim 后 query，并由既有空白分词生成字面量 token。因此纯标点和符号片段可搜索，混合 query 的既有召回与排序语义不变。

所有模型可见文本读取统一使用 1-based `start_line` / `line_count`。省略两者表示从第一行开始尝试全文读取；内容超过工具或 Profile 的既有上下文预算时，调用仍成功并返回前段行预览、实际 `startLine` / `endLine` / `totalLines`、`nextStartLine` 与续读提示。字符数只用于内部预算和统计，不再是模型寻址接口。超长单行超过预算时会明确标记该行截断，不恢复第二套字符分页协议。

`workspace.list_files`

- Read-only。
- 返回 workspace tree。
- 可按 path prefix。

`workspace.search_files`

- Read-only。
- 只搜索 manifest 中模型可见的 workspace roots，例如 `persist/`、`summaries/`、`plan/`、`scratch/`、`output/`。
- 输入为 `query`，可选 `path`、`limit`、`context_lines`。
- 返回 path、score、line range、snippet、sha256 与 `workspace:<path>#Lx-Ly` ref。
- 0 命中是成功结果；非法 path、不可见 path、缺失 path 是 recoverable tool error。
- 不搜索 `input/`、`model-responses/`、`checkpoints/`、`events.jsonl` 等隐藏 runtime 存储；`tool-results/` 是唯一显式可见的 runtime result root。

`workspace.read_file`

- Read-only。
- 只能读 visible resource。
- 只支持 1-based `start_line` / `line_count` 行范围；省略时默认全文，超限时自动返回可续读预览。
- 完整读取会记录完整 read-state；部分读取会记录实际读到的文本片段，允许 `workspace.apply_patch` 替换该片段中出现的唯一 `old_string`。
- 受内部 byte 上限、line 与 partial char 上限控制。
- 非 UTF-8 文件返回 recoverable `workspace.file_not_text`，由模型选择其他文本资源；真实 IO、安全和一致性错误仍 fail-fast。

`workspace.write_file`

- Mutating。
- 只能写 writable path；return-mode child 应把 `summaries/` / `scratch/` 用作私有笔记，只在任务要求 artifact 或 edit 时写共享 writable roots。
- `mode` 默认为 `replace`；`append` 把 `content` 原样追加到文件末尾，文件不存在时创建文件。
- `append` 不自动补换行；需要另起一行时，模型应在 `content` 开头包含 `\n`。
- 写后应 checkpoint。
- `replace` 写已存在文件若发生并发修改，会返回 recoverable stale-file tool error；模型重新读取后再写。

`workspace.apply_patch`

- Mutating。
- 应使用明确 patch 格式；编辑已有文件前必须先读到要替换的精确文本，`replace_all=true` 必须完整读取。
- patch 失败返回可恢复 tool error；如果失败发生在部分读取基础上，同文件再次 patch 前必须完整读取。
- path escape 是 system failure。

`workspace.create_checkpoint`

- Mutating metadata。
- 可由 runtime 自动调用，也可暴露给模型。

`workspace.commit`

- 控制/Mutating 工具。
- 无参数默认将 `output/main.md` 以 `replace` 提交到当前 chat message。
- `append` 在本 run 无既有 commit 时创建消息，之后始终追加同一消息楼层。
- 实际 chat 写入必须通过前端 host bridge 调用上游 `saveReply()`。

`workspace.finish`

- 控制工具。
- 表示模型认为本次 run 已完成。
- 前台 run 在 finish 前必须至少成功 `workspace.commit` 一次；后台 run 可以无 commit。
- Runtime 在 finish 收尾阶段提交 `persist/` projection。
- return-mode child invocation 不可用；child 必须使用 `task.return`。
- 当前允许在 unfinished child task 存在时结束 root run；finish 会默认取消当前 parent 拥有的 unfinished child tasks，run 收尾会取消剩余 unfinished child tasks。

### 7.4 Chat Tools

`chat.search`

- Read-only。
- 通过 Rust chat repository / group chat repository search 能力实现。
- 只能搜索当前 run 绑定的聊天，不允许模型传入任意 chat target。
- 只有 `query` 必填；`limit`、`role`、`start_message`、`end_message`、`scan_limit` 都是可选参数。
- 返回 message index、role、score、snippet 与 `chat:current#<index>` ref。
- 0 命中是成功结果，不是 recoverable error。
- 不能把完整 history 拉入前端。

`chat.read_messages`

- Read-only。
- 通过 0-based message index 精确读取当前聊天消息。
- 输入使用 `messages: [{ index, start_line?, line_count? }]`，与其他文本读取工具保持同一心智模型。
- 一个工具调用最多读取 20 条消息。
- 默认读取完整消息；单条或总结果超出既有上下文预算时返回按行截取的成功预览，并提示下一 `start_line`。
- 总返回上限 20000 字符。
- message index 不存在或范围非法属于 recoverable tool error；正文过大时返回成功的可续读预览。
- chat JSONL header 不计入 message index；第一条聊天消息 index 为 0。

### 7.5 Skill Tools

`skill.list`

- Read-only。
- 返回当前 Profile 可见的已安装 Skill 索引摘要。
- `skills.deny` 优先于 `skills.visible`；`visible: ["*"]` 表示全部已安装 Skill 可见。
- 不读取 `SKILL.md` 全文。

`skill.search`

- Read-only。
- 输入为 `name`、`query`，可选 `path`、`limit`、`context_lines`。
- 只能搜索当前 Profile 可见且未 deny 的单个 Skill。
- 搜索结果只返回 snippet 与 ref，不返回完整文件。
- 返回 snippet 字符数计入 Profile 的 Skill run read budget，防止绕过 `skill.read` 预算。
- 二进制文件会显式计入 skipped files；非法 path、缺失 path、不可见 Skill 或预算耗尽是 recoverable tool error。

`skill.read`

- Read-only。
- 输入为 `name`、可选 `path`、`start_line`、`line_count`。
- `path` 默认 `SKILL.md`，必须是 Skill 内相对路径。
- 默认读取全文；超出 Profile 读取预算时返回按行预览和下一起始行。
- 只能读取 Profile 可见的 UTF-8 文本；二进制、缺失文件、非法路径、symlink escape、不可见 Skill 或超预算读取都是 recoverable tool error，除非 repository 内部 IO / index 损坏等宿主级问题需要 fail-fast。
- Profile 的 `maxReadCharsPerCall` 与 `maxReadCharsPerRun` 继续作为宿主内部读取预算，不暴露为模型范围参数。
- 结果进入 journal / tool result / 下一轮 model request。Skill 原始文件保持 read-only；模型需要摘录或改写时应写入 `scratch/`、`summaries/` 或 `output/`。

### 7.6 WorldInfo Tools

`worldinfo.read_activated`

- Read-only。
- 读取本次 run materialized 的 `promptSnapshot.worldInfoActivation`。
- `startRunFromLegacyGenerate()` 从本轮 dryRun 的最终 `WORLDINFO_SCAN_DONE` 捕获该快照。
- 无参数调用只返回本轮激活条目的索引：`ref`、条目名、世界书名、位置与正文行数/字符数，不返回正文。
- 读取正文传入 `entries: [{ ref, start_line?, line_count? }]`，其中 `ref` 来自无参数索引结果；省略行范围时默认全文，长正文自动返回可续读行预览。
- 模型可读 `content` 保持简洁：索引模式给条目列表，正文模式只给被明确请求的条目内容。
- 结构化结果可以保留 `uid`、`position`、`ref`、`timestampMs` 等 audit 字段，但不要把这些作为模型阅读主内容。
- 不暴露 world info 扫描中间循环状态为 Public Contract。

## 8. Provider Tool Call Adapter

Provider 边界只负责格式转换：

```text
AgentModelTool
  -> provider function schema

provider alias + call id + arguments
  -> ToolInvocation { call_id, tool_id, arguments, provider_metadata }

AgentToolResult.tool_id
  -> current request 的 AgentModelTool
  -> provider result alias
```

alias 只能在当前 request 的 `AgentModelTool` 集合中解析。缺失 call id、未知 alias、历史 ToolId 不在当前 request 或 provider metadata 丢失都必须 fail-fast；不得查询全局 registry、按 native name 猜测或生成 fallback id。

上层 runtime 不应关心 OpenAI、Claude 或 Gemini 的 wire 格式。

## 9. AgentToolResult 进入 Prompt

工具结果不写 chat message。

当前路径：

```text
AgentToolResult
  ↓
AgentModelContentPart::ToolResult
  ↓
下一轮 AgentModelRequest
  ↓
provider adapter projects { ok, content, errorCode, resourceRefs }
```

结果正文可以按真实上下文预算裁剪或转为 resource ref，但 canonical `call_id + tool_id` 不能丢失。`structured` 不属于模型语言界面；未来若出现真正需要 schema-validated structured output 的 provider 契约，应显式增加该契约，不能重新透传内部审计对象。未来需要摘要、可见性或顺序策略时，应在 context/request assembly 边界实现，不修改基础 descriptor。

## 10. Approval（后续统一设计）

当前没有工具审批状态机。MCP Ask 与 Allow 在 Agent/Legacy 中都自动执行；Manager 必须明确提示这一点。审批只能在覆盖全部工具类别的统一交互、journal 与恢复语义设计完成后上线，不能先建立 MCP-only 弹窗。

未来需要纳入审批设计的工具包括：

- MCP tools。
- destructive tools。
- commit/rollback。
- external network side effects。
- 高成本模型/采样工具。

Approval UI 至少展示：

- tool name/title。
- arguments summary。
- side effect annotation。
- source。
- policy reason。

审批结果必须写 journal。

## 11. Error Semantics

工具错误分三类：

```text
RecoverableToolError
  模型可读结果，run 可继续。

PolicyDenied
  根据 policy 决定 fail-fast 或返回 denied tool result。

SystemFailure
  runtime 失败，run 进入 Failed。
```

例子：

- `chat.search` 查不到结果：successful empty result。
- `workspace.apply_patch` patch context mismatch：recoverable error。
- `workspace.read_file` path traversal 参数：recoverable invalid path tool error。
- denied MCP tool：policy denied；是否 recoverable 由该工具的 policy 决定。
- journal append failed：system failure。

## 12. 与 Legacy ToolManager 的关系

当前前端 `ToolManager` 是 Legacy Generate 的工具系统。它：

- 在前端注册工具。
- 直接调用 JS action。
- 把结果保存成 `is_system` chat message。
- 递归调用 `Generate()`。

Agent Tool System 不能复用它作为运行时真相。

可以借鉴：

- function tool 的作者体验。
- display name / format message。
- provider tool schema 注册经验。

禁止继承：

- 工具结果写 chat 楼层。
- 递归 Generate 驱动循环。
- 后端执行任意 JS。

## 13. Extension Tool Bridge

未来扩展工具应通过受控 bridge：

```text
extension registers tool metadata
  ↓
host contributes canonical ToolDescriptor to ToolCatalog
  ↓
snapshot / turn / ToolRequestGate
  ↓
executor router asks frontend bridge to execute
  ↓
AgentToolResult returns to backend journal
```

要求：

- extension tool 默认需要用户或 profile 授权。
- bridge 调用必须有 timeout。
- result 必须结构化。
- extension 不得直接写 Agent workspace，必须通过 tool result 或受控 workspace tool。
- 第二 executor 出现前不建立 router、port 或空 bridge skeleton。

## 14. 当前 Tool System 基线

当前已经具备真正多轮 Agent loop 所需的最小工具系统：

- `ToolDescriptor` / `ToolInvocation` / `AgentToolResult` domain model。
- Rust-owned builtin `ToolCatalog`。
- canonical descriptor 与 snapshot-bound model alias。
- Agent runtime 使用 `AgentModelContentPart::ToolCall(ToolInvocation)` / `ToolResult(AgentToolResult)`，provider 边界负责格式转换。
- chat search/read_messages、worldinfo read_activated、dice roll、skill list/read、workspace list/read/write/apply_patch/finish。
- agent list/delegate/await 与 runtime-only task.return 的 return-mode SubAgent MVP。
- tool arguments / tool results resource refs。
- recoverable tool error 回填模型。
- workspace write/patch 成功结果只向模型回填摘要与 resource refs；内部结构化元数据保留给 runtime、audit 与 Timeline UI。需要完整内容时由模型显式调用 workspace_read_file。
- workspace mutation checkpoint。
- journal events。
- MCP cached descriptor resolution、permission-aware call 与 known/unknown outcome 投影。

新增 extension bridge 工具时，应复用这一套 Catalog/Snapshot/Gate/result/error 语义，只在出现真实第二 executor 需求时增加最小 router。Skill policy 已留在现有 `skill.list` / `skill.search` / `skill.read` dispatcher 与 profile resolver 之间；后续不要新建第二套 Skill 读取入口。
