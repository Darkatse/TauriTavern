# Agent Framework 当前状态

本文档是 Agent 框架的当前事实入口。后续开发先读本文，再读 `docs/AgentArchitecture.md`、`docs/AgentContract.md`、`docs/AgentImplementPlan.md` 与 `docs/Agent/` 下的专题文档。

历史施工说明已经吸收为当前基线，不再作为开发入口；需要历史背景时只看 git history。

## 当前基线

截至 2026-06-06，Agent 当前基线：

- Rust 后端已有 Agent domain model、runtime、workspace、journal、checkpoint、commit bridge。
- 前端已挂载 `window.__TAURITAVERN__.api.agent` Host ABI。
- Agent 启动仍通过 `PromptSnapshot` 兼容桥进入；root run 已支持 Agent Profile 独立 preset 与独立 model 的 Frontend PromptAssemblyBroker 组装，`GenerationIntent + ContextFrame` 尚未完全接管上下文组装。
- LLM 调用仍复用 `ChatCompletionService::generate_exchange_with_cancel()`，不得绕过现有 provider、secret、日志、endpoint policy、iOS policy、prompt cache 或取消链路。Responses WebSocket 建连已收敛到 `HttpClientPool` 的 ChatCompletion WebSocket profile，见 `docs/CurrentState/NativeApiFormats.md`。
- Agent runtime 已不再把 OpenAI-compatible raw JSON 当作内部事实；运行时使用 canonical `AgentModelRequest` / `AgentModelResponse` / `AgentModelMessage` / `AgentModelContentPart`。
- `AgentModelGateway` 在 Agent canonical IR 与现有 `ChatCompletionGenerateRequestDto` 之间转换；provider-native metadata 作为 opaque `Native` part 保留。
- `provider_state` 已是 run-owned、invocation-scoped continuation contract；OpenAI Responses 使用它驱动 persistent WebSocket、incremental input 与 `previous_response_id`。
- Agent Skill 管理、导入导出、embedded skill 提示导入、`skill.list` / `skill.search` / `skill.read` 已落地。
- Phase 3 Agent Profile 基线已落地：`profileId` 会解析为 `ResolvedAgentProfile`，驱动 tools、Skill、workspace roots、output artifact、tool budget、max rounds 与 model-facing prompt/tool descriptions。
- PromptManager 已为 Agent Mode 提供 `agentSystemPrompt` 组装位置与 reserved no-op `agentResults` 位置标记；`agentSystemPrompt` 内容只由 Agent Profile 提供，前端在该 PromptManager index materialize，runtime 只消费最终 messages 并拒绝内部 marker 泄漏；`agentResults` 不再向模型注入历史 commit 内容。
- Profile 已能通过 `preset.mode = "ref"` 使用独立 OpenAI/chat-completion preset，并通过 `model.mode = "connectionRef"` + `modelId` 使用独立 LLM Connection。当前完整 PromptAssemblyBroker 组装覆盖 root run 启动前、return-mode child invocation 与 handoff invocation；运行中 invocation 仍走 host bridge handshake，Rust runtime 不手写替代 PromptManager。
- Return-mode SubAgent MVP 已落地：root/active invocation 可通过 `agent.list`、`agent.delegate`、`agent.await` 创建、查看或等待子任务；child invocation 使用 `task.return` 结束，不能直接 `workspace.commit`、`workspace.finish` 或 `agent.handoff`。当前 child task 已由 run-scoped scheduler 后台并行执行；`agent.await` 只等待/查询结果，未显式 await 的 terminal results 会在父 Agent 下一次 tool turn 后注入下一轮模型请求。
- Handoff MVP 已落地：具备 `delegation.canHandoff` 且显式暴露 `agent.handoff` 的 foreground owner 可把同一 `AgentRun` 的下一阶段交给 `allowAsHandoffTarget` 的 target Profile。handoff 创建 `TransferControl` task 与 `Handoff` invocation，不进入后台 scheduler；当前 invocation 记为 `Transferred` 后，executor 在同一 run 内继续目标 Agent 的工具循环。已经 `workspace.commit` 过的 Agent 仍允许 handoff，最终 handoff owner 可继续修订、再次 commit 并 `workspace.finish`。实现框架、结构体与事件序列见 `docs/Agent/Handoff.md`。
- 当前工具循环是非 streaming；provider stream 仍不是 Agent timeline event。
- Agent System 扩展开关开启时，当前前端会把普通发送、regenerate 与 overswipe 新候选生成接入 Agent；实际 root run 使用扩展设置中的 `activeProfileId`。Profile 面板的 `editingProfileId` 只表示当前正在编辑的配置档案，不影响生成。Agent Mode off 时上游 SillyTavern 生成、事件和保存语义不变。
- Agent System 前端已提供 run timeline / detail panel；timeline 以 `readEvents(beforeSeq, limit)` 读取最新页并按需补拉更早 journal 页，默认视图只投影用户可见操作，DOM 使用窗口化渲染避免长 run 在低端移动设备上堆积节点；详情面板顶部可拖动调整高度，高度仅作为扩展 UI 偏好保存，不进入 Agent Host ABI、journal 或 Rust runtime。前端实现将 timeline 数据会话、projection 规范化、详情读取、显示格式与 Vue 外壳拆分：运行中贴片、历史回看弹窗与 SubAgent 局部 timeline 共享同一 session/detail primitives，但各自仍通过 Host ABI 表达运行态、只读历史态与 `invocationId` 局部事件页。
- Agent run history 已提供只读 `listRuns()` / `list_agent_runs(dto)` 入口；首版从 `_tauritavern/agent-workspaces/index/runs/*.json` 读取 `AgentRun` 摘要，支持 `chatRef`、`stableChatId`、`statuses`、时间游标与 limit。列表会维护 `_tauritavern/agent-workspaces/index/run-summaries/<run-id>.json` projection，从 `chat_commit_completed.messageId` 派生提交当时的 `messageIndex`；该索引是提交时快照，不扫描聊天消息、不反查当前楼层。只有已经写入 terminal event 的终态 run 会复用/落盘 summary projection，运行中或终态事件尚未出现的窗口只即时投影。
- Agent run retention policy 已进入 `tauritavern-settings.agent.retention`：`auto_prune_enabled` 默认 `false`，控制 Rust 后端是否在 TauriTavern 进程运行期间周期性执行自动清理；`keep_recent_terminal_runs` 默认 100，表示保留最近 terminal run 的核心历史；`keep_full_recent_runs` 默认 20，表示其中保留完整 workspace/debug artifacts 的窗口。两个数量允许为 0，最大 10000，且 full 窗口必须是 history 窗口子集。前端 Host ABI 已提供 `api.agent.retention.readSettings()` / `updateSettings()` / `planPrune()` / `applyPrune()` facade，Agent System Runs 面板可保存策略、显示 dry-run 清理预览，并在用户确认后执行手动清理。后端已提供 `plan_agent_run_prune(dto)` 与 `apply_agent_run_prune(dto)` command，用当前设置或一次性 override 生成并执行 `slim_heavy_artifacts` / `delete_run` 候选；`apply` 使用显式 execution plan 模式，在后端重新规划全量候选，不信任前端预览明细，且同一服务实例内串行执行。自动清理由 `AgentRunRetentionAutomationService` 拥有生命周期，不依赖 Agent System 面板打开；保存 retention 设置只唤醒调度器重读配置，不同步执行清理。`slim_heavy_artifacts` 基于 Agent run storage class，路径归属词汇与 TT-Sync Agent run datasets 对齐，但 retention 不依赖同步 profile。明细受 `detailLimit` 限制但 totals 不截断，active run、缺失 terminal event、journal/storage 异常会进入 `blockedRuns`；单个执行失败会进入 `failedRuns`。
- 输入框 Agent 快捷开关 `#ttas_agent_send_toggle` 可以在 Agent System 扩展抽屉中隐藏；该状态只是扩展 UI 偏好，不改变 `agentModeEnabled`、生成路由、Agent Host ABI、journal 或 Rust runtime。

## 当前 Host ABI

已落地：

```ts
api.agent.startRunFromLegacyGenerate(input?)
api.agent.startRunWithPromptSnapshot(input)
api.agent.subscribe(runId, handler, options?)
api.agent.cancel(runId)
api.agent.listRuns(input?)
api.agent.retention.readSettings()
api.agent.retention.updateSettings(input)
api.agent.retention.planPrune(input?)
api.agent.retention.applyPrune(input?)
api.agent.readEvents(input)
api.agent.readWorkspaceFile(input)
api.agent.readModelTurn(input)
api.agent.promptAssembly.prepare(input)
api.agent.promptAssembly.buildSnapshot(input)
api.agent.profiles.list()
api.agent.profiles.load(input)
api.agent.profiles.diagnose(input)
api.agent.profiles.resolveSystemPrompt(input?)
api.agent.profiles.save(input)
api.agent.profiles.delete(input)
api.agent.tools.list()
api.llmConnections.list()
api.llmConnections.load(input)
api.llmConnections.save(input)
api.llmConnections.delete(input)
```

`startRunFromLegacyGenerate()` / `startRunWithPromptSnapshot()` 支持可选 `profileId`。Profile 管理、工具列表与 prompt assembly broker API 已封装到 `window.__TAURITAVERN__.api.agent` 与 `src/types.d.ts`；LLM Connection 管理已封装到 `window.__TAURITAVERN__.api.llmConnections`。Agent Profile 面板已提供独立 preset / model 选择，model 选择以 Connection Manager 的 Model Target 作为 UI 输入来源，保存时物化为 Agent domain 的 LLM Connection，并在 Profile 中持久化 `connectionRef + modelId`。

Skill 管理 API 已落地：

```ts
api.skill.list()
api.skill.previewImport(input)
api.skill.installImport(request)
api.skill.readFile(input)
api.skill.writeFile(input)
api.skill.move(input)
api.skill.export(input)
```

`api.skill` 是用户/UI/扩展侧的 Skill 管理入口；Agent run 内只通过 `skill.list` / `skill.search` / `skill.read` 工具消费已安装 Skill。

`readModelTurn()` 读取指定 run/round/invocation 的模型回合显示 DTO：assistant 输出、narration、可见/摘要化 reasoning、工具调用摘要与 provider 摘要；`invocationId` 省略时读取 root invocation。前端 Timeline 不直接解析 `model-responses/` raw 文件。

明确不存在公共 `api.agent.startRun()` alias。启动入口必须表达 prompt 来源：

- `startRunFromLegacyGenerate()`：调用 Legacy `Generate(..., dryRun = true)`，捕获 `GENERATE_AFTER_DATA` 中的 `generate_data` 与本轮最终 `worldInfoActivation`。
- `startRunWithPromptSnapshot()`：调用方已经持有 `promptSnapshot.chatCompletionPayload`，可选携带 `promptSnapshot.worldInfoActivation`。

当前显式拒绝：

- `stream: true`
- prompt snapshot 中已有 external `tools`
- external `tool_choice`
- 已有 `role: "tool"` 或 assistant `tool_calls`

## Agent Profile 当前事实

Profile 使用 JSON 文件，存储于：

```text
_tauritavern/agent-profiles/
  profiles/<profile-id>.json
  .staging/
```

当前实现边界：

- 缺省 `profileId` 使用 built-in `default-writer`。
- 非缺省 `profileId` 不存在时 fail-fast，不创建 run。
- `instructions.agentSystemPrompt` 省略或为 `null` 时使用 resolved profile 默认 Agent system prompt；设置为非空字符串时完整替换默认 prompt；空白字符串 fail-fast。Preset 控制 `agentSystemPrompt` 的位置与 role，不能编辑其内容。
- `preset.mode = "ref"` 会加载指定 OpenAI/chat-completion preset，经 Frontend PromptAssemblyBroker 真实复用 SillyTavern PromptManager 组装；`currentPromptSnapshot` / `none` 保留兼容路径。
- `model.mode = "connectionRef"` 要求 `connectionRef` 与 `modelId`，组装阶段会把 source/model 覆盖到 prompt settings，runtime 发送前会再次以 LLM Connection 权威覆盖 payload。
- `model.mode = "requiresConfiguration"` 表示外部导入/分享后的 Profile 需要本机重新选择模型；该状态可保存、可展示，但 root run、SubAgent 与 prompt assembly 都会 fail-fast。
- `context.initialChatHistoryMessages` 只作用于 PromptManager 组装期的初始聊天历史窗口：`-1` 不做显式楼数裁剪，`0` 不向初始 prompt 注入真实聊天楼层，正数最多注入最近 N 楼。`FrozenRunInputSnapshot.promptInputs.messages` 保留 SillyTavern PromptManager 的 latest-first 完整候选历史；组装期只能 materialize 工作副本后裁剪、注入和反转，不能污染 frozen input，便于 child / handoff 按 target Profile 重新组装；`chat.read_messages`、`chat.search` 与 persist base 仍按 Rust runtime 的 run input prefix 工作。
- `tools.allow` / `tools.deny` 决定模型可见工具，dispatcher 会二次拦截不可见工具。
- `tools.toolDescriptions` 省略或为空时使用默认工具 description；设置时只替换 model-facing ToolSpec copy 的工具总 description 与参数 description。
- `skills.visible` / `skills.deny` 控制 `skill.list`、`skill.search` 与 `skill.read`，`maxReadCharsPerCall` / `maxReadCharsPerRun` 控制 Skill 读取预算。
- 每个 invocation 按 `global -> preset -> profile -> character` 解析 active Skill scopes。root run 会固化 ambient `skillScopeRefs`；return-mode child 使用 target Profile 的 Skill policy，并按 target preset / run ambient character 解析可读 Skill。
- `workspace.visibleRoots` / `workspace.writableRoots` 只能收窄 root universe：`output`、`scratch`、`plan`、`summaries`、`persist`。
- `run.presentation` 区分 `foreground` / `background`，默认 built-in profile 为前台；`run.directRunnable` 控制 Profile 是否可被用户直接启动。直接可运行 Profile 必须暴露 `workspace.finish`，前台直接运行还必须暴露 `workspace.commit`；非直接运行 Profile 必须可作为 return-mode SubAgent 或 handoff target。仅作为 handoff target 的前台 Profile 可以不暴露 `workspace.finish`，只要它仍能继续 handoff；若要成为最终收尾 Agent，则应暴露 `workspace.commit` / `workspace.finish`。
- `profiles.list()` 的 summary 暴露 `directRunnable`，Agent System UI 只允许直接可运行 Profile 成为 `activeProfileId`。保存或删除当前生效 Profile 导致其不可直接运行时，前端会把 `activeProfileId` 显式切回 built-in `default-writer`；不会把当前正在编辑的 Profile 自动设为生效。
- `profiles.diagnose()` 返回管理态 `AgentProfileHealth`，用于展示可加载 Profile 的外部资源健康度。第一期覆盖 missing/unsupported preset ref、`model.requiresConfiguration`、LLM Connection 缺失或无效；该 API 不改变 run / prompt assembly 的 fail-fast 语义，也不会让运行静默回退当前 UI preset/model。
- `run.modelRetry` 控制单次模型调用的瞬时错误重试；默认 `maxRetries = 3`、`intervalMs = 3000`。当前只重试 rate limit / transient transport-provider 错误，不重试 prompt/schema/native metadata/tool id 等契约错误。
- `delegation` 控制多 Agent 能力：`canDelegate` 决定当前 Agent 是否可见 `agent.delegate` / `agent.await`，`callable`、`allowAsSubagent`、`allowedCallers` 决定该 Profile 是否可被其他 Agent 作为 return-mode SubAgent 调用；`canHandoff` 决定当前 Agent 是否可见 `agent.handoff`，`allowAsHandoffTarget`、`allowedCallers` 与 `maxHandoffDepth` 决定该 Profile 是否可作为接力目标以及接力深度上限。
- `output.artifacts` 当前必须包含且只能包含一个 `messageBody` artifact；`workspace.commit` 默认发布该 artifact 的 path。
- Plan Mode schema 已存在，但当前只支持 `plan.mode = "none"`；其他 mode fail-fast。
- 每个 run 会在 `input/resolved_profile.json` 固化解析结果。

## 当前工具集

Tool registry 只产 canonical `AgentToolSpec`。Provider-facing alias 由 gateway/payload adapter 渲染，不再由 registry 暴露 OpenAI-shaped tools。

Agent run 创建时，Rust runtime 会冻结本 run 的输入历史前缀：`swipe` 排除当前最后一条 assistant 目标楼层，`regenerate` 排除最后一条非 user 楼层。`chat.search`、`chat.read_messages` 与 persistent state base 解析都只消费这个前缀；这是 runtime 内部语义，不进入 model-facing tool description。

| Canonical name | Model alias | 类型 | 当前语义 |
| --- | --- | --- | --- |
| `agent.list` | `agent_list` | read-only | 列出当前 Profile policy 允许调用的 Agent 目录；用于软渐进式披露可委派 Agent。 |
| `agent.delegate` | `agent_delegate` | control/mutating | 创建 return-mode 子任务与 child invocation，并提交 run-scoped scheduler 后台执行。 |
| `agent.handoff` | `agent_handoff` | control/mutating | 创建 TransferControl task 与 handoff invocation，让目标 Agent 接手同一 run 的下一阶段；调用方 handoff 后必须停止继续调用工具。 |
| `agent.await` | `agent_await` | read-only/control | 查询或等待当前 invocation 创建的 delegated task；不驱动 child task 执行。 |
| `task.return` | `task_return` | control/mutating | runtime-only child invocation 工具，提交 delegated task 结果并结束 child work。 |
| `chat.search` | `chat_search` | read-only | 搜索当前 run 绑定的聊天。只有 `query` 必填；可选 `limit`、`role`、`start_message`、`end_message`、`scan_limit`。 |
| `chat.read_messages` | `chat_read_messages` | read-only | 按 0-based message index 读取当前聊天消息；每项可选 `start_char`、`max_chars`。JSONL header 不计入 index。 |
| `worldinfo.read_activated` | `worldinfo_read_activated` | read-only | 读取本次 Agent run 捕获的最终激活世界书条目，不读取全局 last activation。 |
| `dice.roll` | `dice_roll` | read-only | 为明确的随机、跑团或 roleplay 检定投骰；支持 `d6`、`1d20`、`3d6+4` 与纯数字。默认 Profile 不启用。 |
| `skill.list` | `skill_list` | read-only | 列出当前 Profile 可见的已安装 Skill 索引摘要。 |
| `skill.search` | `skill_search` | read-only | 搜索当前 Profile 可见的单个 Skill 内 UTF-8 文本文件；返回 snippet/ref，snippet 字符数计入 Skill read budget。 |
| `skill.read` | `skill_read` | read-only | 读取当前 Profile 可见 Skill 内的 UTF-8 文本文件或范围；默认 `SKILL.md`，支持 `path`、行范围、字符范围与 `max_chars`，受 Profile read budget 控制。 |
| `workspace.list_files` | `workspace_list_files` | read-only | 列出模型可见 workspace 文件。`path` 省略、空字符串、`.`、`./` 表示 workspace root。 |
| `workspace.search_files` | `workspace_search_files` | read-only | 搜索模型可见 workspace UTF-8 文本文件；可限定 `path`，返回 snippet/ref，不搜索隐藏 runtime 存储。 |
| `workspace.read_file` | `workspace_read_file` | read-only | 读取 UTF-8 文本文件并返回行号；支持行范围和字符范围；完整读取会记录 read-state。 |
| `workspace.write_file` | `workspace_write_file` | mutating | 写 UTF-8 文件；`mode` 默认为完整替换，`append` 会原样追加并在缺失时创建文件；成功后按内容可知性更新 read-state 并创建 checkpoint。 |
| `workspace.apply_patch` | `workspace_apply_patch` | mutating | 单文件 `old_string` / `new_string` 精确替换；`old_string` 必须来自本 run 已读文本或本 run 创建/完整替换的文件；失败后同文件再次 patch 必须先全文读取。 |
| `workspace.commit` | `workspace_commit` | control/mutating | 将可见 workspace 文件提交到当前聊天；无参数等价于 `replace output/main.md`，`append` 首次创建消息、后续追加同一消息。 |
| `workspace.finish` | `workspace_finish` | control | 结束 root/active 工具循环；前台 run 必须已有至少一次 run-level 成功 commit，后台 run 可无 commit；return-mode child invocation 不可用；当前会取消 unfinished return-mode child tasks 而不阻塞完成。 |

当前没有 MCP 工具、shell 工具、外部 extension tools、tool approval、profile routing 或模型可见 task cancel。

## Model Gateway 当前事实

当前 Agent model 边界：

```text
AgentRuntimeService
  -> AgentModelGateway.generate_with_cancel(AgentModelRequest)
    -> encode_chat_completion_request()
      -> ChatCompletionService.generate_exchange_with_cancel(ChatCompletionGenerateRequestDto)
        -> provider payload builder / repository / logging wrapper
    -> decode_chat_completion_response()
  -> AgentModelResponse
```

Canonical IR 位于 domain model：

```rust
AgentModelRequest {
    payload,
    messages,
    tools,
    tool_choice,
    provider_state,
}

AgentModelContentPart {
    Text,
    Reasoning,
    ToolCall,
    ToolResult,
    Media,
    ResourceRef,
    Native,
}
```

当前实现重点：

- `AgentModelGateway` 已拆为 `agent_model_gateway/` 模块目录：`mod.rs` 保留 trait / `ChatCompletionAgentModelGateway` wrapper；`encode.rs` / `decode.rs` 做 canonical IR 与 normalized ChatCompletion exchange 转换；`format.rs` 解析 source / provider format；`schema.rs` 做 tool schema sanitizer；`provider_state.rs` 管理 continuation；`providers/*` 放 provider-specific adapter 规则。
- Agent runtime 只消费 `AgentModelResponse.tool_calls`，不再读 `/choices/0/message/tool_calls`。
- Tool call id 必须是 provider 返回的不透明字符串；缺失 `tool_call_id` 会 fail-fast。
- OpenAI Responses 请求会注入 `include: ["reasoning.encrypted_content"]`，以便保留 reasoning continuation 所需 opaque 内容。
- Tool schema 在 gateway 边界按 provider format 做深拷贝 sanitizer；canonical schema 本身不被污染。
- Claude / Gemini / OpenAI Responses / Gemini Interactions 的 native blocks 会进入 normalized `message.native`，再进入 Agent `Native` part。

当前 `provider_state` 契约：

- 初始值是 `{ "sessionId": runId }`。
- 每轮成功后由 `AgentModelGateway` 返回新的 `provider_state`，包含 `sessionId`、`chatCompletionSource`、`providerFormat`、`messageCursor`、`lastResponseId`。
- OpenAI Responses 额外包含 `transport: "responses_websocket"` 与 `previousResponseId`。
- OpenAI Responses 第二轮起只发送 `messageCursor` 之后的新消息，并过滤 assistant messages；缺失或越界 cursor 会 fail-fast。
- native provider 返回 tool call 但缺失对应 native part 时，以 `model.native_metadata_lost` fail-fast。
- ChatCompletion payload 内部使用 `_tauritavern_provider_state` 传递该状态；LLM API log 与真正发往上游的 payload 都会剥离该字段。
- 完整契约见 `docs/CurrentState/AgentProviderState.md`。

当前 native metadata 保留点：

- Claude：保留 assistant `content` blocks，用于回放 `thinking` / `tool_use` / signature。
- Gemini：保留 response `content.parts` 与 `thoughtSignature`。
- OpenAI Responses：保留 raw `output` items 与 `responseId`。
- Gemini Interactions：保留 raw `outputs`，包含 `thought`、`function_call`、URL context 等非通用块。

## Tool Result Context 策略

当前工具结果有两个面：

- journal / workspace 保存的是真实 tool result、tool args、resource refs。
- 下一轮模型上下文使用 canonical `ToolResult` part。

当前不再自动补入 workspace 写入内容：

- `workspace.write_file` 与 `workspace.apply_patch` 成功结果只把摘要、结构化元数据与 resource refs 作为 canonical `ToolResult` 回填模型。
- runtime 不会因前几轮 workspace 写入而自动读取完整文件内容并拼入下一轮模型上下文。
- 模型需要完整文件内容时必须显式调用 `workspace.read_file`；append 到未完整读过的旧文件后，后续 rewrite / patch 会按 read-state 规则要求重新读取。
- read-state 由 workspace 工具维护：完整读取、创建/完整替换会记录完整已知；部分读取会记录 Agent 实际读到的片段，允许 `workspace.apply_patch` 替换已读片段中的唯一 `old_string`；基于部分读取的 patch 一旦失败，同文件再次 patch 必须先全文读取；append 只有在旧内容已经完整已知时才延续完整已知。

这样让跨 Agent / 跨模型协作依赖显式 task packet、workspace refs 与 invocation-scoped prompt，而不是旧的 provider switch 最近历史启发式。

## Workspace 与 Commit

默认模型可见 / 可写 roots：

```text
output/
scratch/
plan/
summaries/
persist/
```

实际 roots 由 resolved Profile 收窄后写入 run manifest。`persist/` 是 chat workspace 级持久 root 的 run projection。run 中修改 `persist/` 只影响本 run；`workspace.finish` 收尾成功时 promote 回 `chats/<workspace-id>/persist/`。

Chat commit 当前由模型显式调用 `workspace.commit` 触发，并由前端 host bridge 执行：

```text
workspace.commit(path?, mode?)
  -> backend 读取 workspace file / checkpoint
  -> chat_commit_requested event
  -> 前端 saveReply(normal | append | appendFinal)
  -> resolve_agent_chat_commit
  -> workspace.finish 成功提交 persist projection 后
  -> persistent_state_metadata_update_requested event
  -> 前端把 persistStateId 写回同一条 chat message
  -> resolve_agent_persistent_state_metadata_update
```

`mode` 默认为 `replace`；`append` 在本 run 尚无 commit 时创建消息，之后多次 commit 始终更新同一个消息楼层。Commit 必须遵守 SillyTavern/windowed payload 保存契约，不能直接写 chat JSONL。`persistStateId` 只能表示已经落盘的 durable persistent state；`chat_commit_requested` 不携带该字段，partial success 保留的聊天输出不会成为下一轮可复用 persist base。下一轮 run 的 `persistBaseStateId` 由 Rust runtime 从同一个输入历史前缀内解析，前端不再负责扫描聊天历史来决定 base state。

聊天删除现在会联动清理对应的 Agent chat workspace：

- 单个角色聊天删除会按 `chat_metadata.integrity` 派生 workspace id 并删除 `_tauritavern/agent-workspaces/chats/<workspace-id>/`。
- 单个群聊聊天删除会按 group chat id 派生 workspace id 并删除对应 workspace。
- 删除角色且选择删除聊天、删除群组时，会批量清理被删除聊天对应的 Agent workspace。
- 若目标 workspace 仍有当前进程中的 active Agent run，删除会 fail-fast，要求先取消 run；不会先删聊天再留下运行中的 workspace。
- 非 Agent / 旧聊天没有稳定 `integrity` 时不产生 Agent workspace 清理目标，以保持 SillyTavern 删除语义。

## 当前 Run Flow

```text
api.agent.startRunFromLegacyGenerate(input?)
  ↓
Legacy dryRun 捕获 generate_data 与 worldInfoActivation
  ↓
api.agent.startRunWithPromptSnapshot(input)
  ↓
前端解析 chatRef / stableChatId
  ↓
start_agent_run(dto)
  ↓
AgentRuntimeService::start_run()
  ↓
resolve Profile
  ↓
创建 AgentRun / workspaceId / run workspace
  ↓
initialize_run 写 manifest / prompt snapshot / resolved profile / persist projection
  ↓
prepare_agent_tool_request 按 Profile 生成 AgentModelRequest 与 model-facing tool specs
  ↓
model -> tool -> model -> ... -> workspace.commit? -> workspace.finish
  ↓
workspace mutation 成功后 checkpoint
  ↓
workspace.commit 成功后 host 写入同一条 chat message
  ↓
workspace.finish 结束 run，并提交 persist projection
```

工具循环轮数来自 `profile.tools.maxRounds`。超过后以 `agent.max_tool_rounds_exceeded` 失败。模型直接输出文本且不调用工具会触发 soft drift recovery：runtime 将直接文本捕获到当前 messageBody artifact root 下的 `direct_output.md`（默认 `output/direct_output.md`），记录 `direct_output_captured` 与 checkpoint，然后提醒模型通过当前 invocation 的 Agent 工具提交/结束。direct output recovery 不再有独立的一次性上限；只要仍有下一轮模型调用预算就继续纠偏，直到恢复、取消或在 `maxRounds` 边界以 `model.tool_call_required` 失败 / `run_partial_success` 收口。前台 run 在 `workspace.finish` 前必须至少成功 `workspace.commit` 一次；后台 run 可以无 chat commit 结束。

Return-mode SubAgent 当前流程：

```text
root/active invocation calls agent.delegate
  ↓
create AgentTaskRecord + child AgentInvocation(TaskReturnRequired)
  ↓
AgentTaskScheduler spawns child worker
  ↓
child model loops with task.return-only exit policy
  ↓
task.return writes agent-results/<child-invocation-id>.json and summaries/<workspace-key>-result.md
  ↓
root/active invocation may call agent.await to wait for selected results
  ↓
otherwise terminal child results are injected after the next parent tool turn
```

Child invocation 与请求它的 Agent 使用同一套逻辑 workspace path；runtime 只按 target Profile 的 `workspace.visibleRoots` / `workspace.writableRoots` 调整当前 invocation 的 root 权限，不再做 child 专用路径映射。完整契约见 `docs/Agent/SubAgent.md`。

## 当前 Run Events

已落地事件包括：

```text
run_created
profile_resolved
generation_intent_recorded
status_changed
workspace_initialized
persistent_projection_initialized
context_assembled
agent_invocation_created
agent_invocation_started
agent_invocation_completed / agent_invocation_failed / agent_invocation_cancelled / agent_invocation_transferred
agent_task_registered
agent_task_queued / agent_task_started / agent_task_completed / agent_task_failed / agent_task_cancelled
agent_delegate_started
agent_await_started
agent_await_completed
task_return_completed
model_request_created
model_call_attempt_started
model_call_attempt_failed
model_call_retry_scheduled
model_completed
direct_output_captured
tool_call_requested
tool_call_started
tool_result_stored
tool_call_completed / tool_call_failed
workspace_file_written
workspace_patch_applied
checkpoint_created
provider_state_updated
model_response_stored
agent_loop_finished
chat_commit_started
chat_commit_requested
chat_commit_completed / chat_commit_failed
chat_commit_recorded
persistent_changes_committed / persistent_changes_commit_failed
persistent_state_metadata_update_requested
persistent_state_metadata_updated / persistent_state_metadata_update_failed
run_completed
run_partial_success
run_cancel_requested
run_cancelled
run_failed
```

Provider stream chunk 不是 Agent run event。Agent UI 必须订阅 `api.agent.subscribe(runId, handler)` 的 run event。

`model_completed` payload 当前包含 `round`、`modelResponsePath`、`toolCallCount`、assistant/reasoning 字节摘要与 `hasAssistantText` / `hasReasoning`。带工具调用且存在可展示 assistant visible text 的模型回合会额外携带可选 `narration` preview；它是模型轮次展示投影，不是 runtime status，不从 reasoning / thinking / thought 提取。工具相关事件携带同一 `round`，便于 UI 从任意工具事件跳回本轮模型回合。

`run_partial_success` 是 warning 级终态：当 run 已经有 host-confirmed `workspace.commit`，但之后因 drift、dispatch、persistent commit 或 persistent metadata 写回错误未能干净完成时，保留已提交 chat 输出，并在 payload 中暴露原始错误与 `preservedCommits`。它不是 `run_completed`，也不触发自动 rollback。partial success 消息不会带可复用的 `persistStateId`；下一轮 Agent run 会跳过它，继续寻找更早的 committed persistent state。

## 当前文件布局

```text
_tauritavern/agent-workspaces/
  index/
    runs/
      <run-id>.json
  chats/
    <workspace-id>/
      persist/
        <promoted persistent files>
      runs/
        <run-id>/
          manifest.json
          events.jsonl
          input/
            prompt_snapshot.json
            resolved_profile.json
            persist_snapshot.json
          invocations/
            inv_root.json
            inv_<child>.json
          tasks/
            task_<id>.json
          tool-args/
            call_<sha256_8byte_hex(tool-call-id)>.json
          tool-results/
            call_<sha256_8byte_hex(tool-call-id)>.json
          agent-results/
            inv_<child>.json
          model-responses/
            round-XXX.json
          output/
            main.md
          scratch/
            agents/
              <workspace-key>/
          plan/
          summaries/
            agents/
              <workspace-key>/
                result.md
          persist/
          checkpoints/
            <checkpoint-id>/
              checkpoint.json
              <snapshotted files...>
_tauritavern/skills/
  installed/
    <skill-name>/
      SKILL.md
      <skill files...>
  index/
    skills.json
  .staging/
_tauritavern/agent-profiles/
  profiles/
    <profile-id>.json
  .staging/
```

Workspace path 必须是相对路径。绝对路径、Windows drive prefix、NUL、`..` 会被拒绝。工具参数层可修正的问题返回 recoverable tool error；repository 内部 IO、journal、checkpoint、chat JSONL 损坏、序列化、取消和模型响应结构错误是 fatal runtime error。

工具参数与结果的审计文件名使用 provider `tool_call_id` 的 SHA-256 前 8 字节 hex 派生；原始 `tool_call_id` 仍保存在 event payload、审计 JSON 内容与下一轮模型 tool result 中，不能被截断、清洗或替换。

## 当前手动测试入口

Agent System 扩展已在输入栏提供 Agent Mode toggle。开启后，普通发送、`/trigger`、regenerate 菜单与右划 overswipe 生成新候选会走 Agent；普通切换已有 swipe 候选仍保持 Legacy swipe 行为。

`/trigger` 仍保持 SillyTavern 的 `normal` generation 语义，不新增 generation type；但 Agent 路由错误必须 fail-fast，不得回退 Legacy Generate。

前端控制台入口：

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);

const agent = window.__TAURITAVERN__.api.agent;

const run = await agent.startRunFromLegacyGenerate({
  generationType: 'normal',
  // profileId: 'default-writer',
  options: { stream: false, presentation: 'foreground' },
});

const stop = agent.subscribe(run.runId, event => console.log(event));
```

`startRunWithPromptSnapshot()` 仍可用于低层 smoke，但不要注入 `tools`、`tool_choice` 或已有 tool turns。

## 最近验证命令

最近一次 Rust 侧验证基线：

- `cargo fmt --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml skill_scope --lib`：2 passed
- `cargo test --manifest-path src-tauri/Cargo.toml agent_runtime_service --lib`：53 passed
- `cargo test --manifest-path src-tauri/Cargo.toml agent_delegate_await_runs_return_mode_subagent`
- `cargo test --manifest-path src-tauri/Cargo.toml child_workspace_policy_scopes_manifest_roots_without_mapping --lib`：1 passed
- `cargo test --manifest-path src-tauri/Cargo.toml file_agent_repository --lib`：10 passed
- `cargo test --manifest-path src-tauri/Cargo.toml file_agent_profile_repository`：1 passed
- `git diff --check`

最近一次前端侧验证：

- `pnpm run check:frontend`
- `pnpm run check:types`
- `pnpm run check:contracts`：218 passed
- `git diff --check`

## 已知待办

- 将 `PromptSnapshot` 过渡输入逐步替换为 `GenerationIntent + ContextFrame`。
- 将 Profile overlay 扩展到 preset / character resolver。
- 继续完善运行中 prompt assembly 的恢复/诊断体验；当前 child / handoff 已有独立 provider_state、model binding 与 host bridge prompt assembly handshake。
- 实现模型可见 task cancel 与更完整的 scheduler policy。
- 明确多 Agent provider/model switch policy；root run 的 `connectionRef` 模型绑定已经可用。
- 实现 readDiff、rollback、resume-run、streaming 的明确策略。

## 每次 Agent 相关变更必须更新

新增或修改 Agent 相关实现时，请同步：

- `docs/CurrentState/AgentFramework.md`
- `docs/CurrentState/AgentProviderState.md`
- `docs/AgentImplementPlan.md`
- `docs/Agent/LlmGateway.md`
- `docs/Agent/PromptAssembly.md`
- `docs/Agent/ToolSystem.md`
- `docs/Agent/Skill.md`
- `docs/Agent/RunEventJournal.md`
- `docs/Agent/TestingStrategy.md`
- 涉及 Host ABI 时同步 `docs/API/Agent.md`、`docs/API/Skill.md`、`docs/API/LlmConnections.md`、`docs/FrontendHostContract.md`、`src/types.d.ts`

## 守护契约

- Agent Mode off 时 Legacy `Generate()` 行为不变。
- LLM 调用不绕过 `ChatCompletionService`、LLM API log、secret、iOS policy、prompt cache；Responses WebSocket 必须继续复用 `HttpClientPool`，不得扩散成新的并行 LLM 调用链。
- Agent runtime 使用 canonical model IR，不把 provider native format 当内部业务事实。
- Provider native metadata 不解析、不清洗、不改写；丢失必要 native metadata 必须 fail-fast 或测试失败。
- Tool call id 是不透明字符串。
- Agent 工具结果不写入 chat 楼层。
- Agent run/timeline event 不伪装成 SillyTavern `GENERATION_*` / `TOOL_CALLS_*` 事件。
- Commit/rollback 遵守 windowed payload 与保存串行化契约。
- MCP stdio command 不由 Agent/Preset/角色卡/世界书直接写入。
