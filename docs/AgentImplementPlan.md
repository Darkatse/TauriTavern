# TauriTavern Agent Implementation Plan

本文档把 Agent 架构拆成可合并、可验证、低技术债的阶段。阶段顺序按重要性和依赖关系排列。

原则：

- 先边界，后能力。
- 先 journal/workspace/commit，再 tool loop。
- 先内置高 ROI 工具，再 MCP/extension tool。
- 先兼容旧 prompt，再逐步后移 Context Assembly。
- 每个阶段都必须有可运行验收，不接受“只搭了一半抽象”的合并。

## Phase 0：文档、契约、测试守护

目标：先把实现者不能越过的边界写清楚。

产物：

- `docs/AgentArchitecture.md`
- `docs/AgentContract.md`
- `docs/AgentImplementPlan.md`
- `docs/Agent/README.md`
- `docs/Agent/Workspace.md`
- `docs/Agent/RunEventJournal.md`
- `docs/Agent/ProfilesAndPreset.md`
- `docs/Agent/ToolSystem.md`
- `docs/Agent/LlmGateway.md`
- `docs/Agent/McpSkill.md`
- `docs/API/Agent.md`
- `docs/API/MCP.md`
- `docs/Agent/TestingStrategy.md`
- `docs/FrontendHostContract.md` 增补 `api.agent` / `api.mcp`
- `docs/API/README.md` 增补 Agent/MCP API 索引
- `docs/CurrentState/AgentFramework.md` 增补实时开发进度跟踪入口

验收：

- 文档之间边界清晰，不重复定义同一概念的不同版本。
- 所有重要约束都能追溯到现有 Ground of Truth 或 Agent 设计目标。
- 明确 Agent Mode off 的回归守护项。

ROI：极高。  
风险：低。

## Phase 1：Workspace + Journal + One-Step Agent

目标：建立 Agent Runtime 骨架，但不急着做多轮工具循环。

范围：

```text
start_agent_run command
resolve stableChatId
create run workspace
materialize minimal input
append events.jsonl
single model call
write output/main.md
create checkpoint
assemble artifact
commit chat message
read run events
cancel before/while model call
minimal timeline UI
```

后端建议：

- 新增 domain models：
  - `AgentRun`
  - `AgentRunStatus`
  - `AgentRunEvent`
  - `WorkspacePath`
  - `WorkspaceManifest`
  - `ArtifactSpec`
  - `Checkpoint`
- 新增 repository traits：
  - `AgentRunRepository`
  - `WorkspaceRepository`
  - `CheckpointRepository`
- 新增 application services：
  - `AgentRuntimeService`
  - `WorkspaceService`
  - `ArtifactAssemblyService`
  - `AgentCommitService`
- 新增 infrastructure：
  - file-backed run repository
  - file-backed workspace repository
  - snapshot checkpoint repository
- 新增 presentation：
  - `agent_commands.rs`
  - `start_agent_run`
  - `cancel_agent_run`
  - `read_agent_run_events`
  - `read_agent_workspace_file`

前端建议：

- 新增 `src/tauri/main/api/agent.js`。
- 在 `bootstrapTauriMain()` 安装 `installAgentApi(context)`。
- `window.__TAURITAVERN__.api.agent.startRunWithPromptSnapshot()` 先通过 `api.chat.open(chatRef).stableId()` 解析 `stableChatId`，再调用 Tauri command。
- 最小 timeline UI 可先只显示状态、model delta、checkpoint、commit。

LLM 调用策略：

- 第一阶段优先复用现有 `ChatCompletionService`。
- 输入可以来自 `PromptSnapshot`，由前端 dryRun 生成；但 dryRun 不是纯函数，只能作为兼容桥。
- 该阶段不重写 provider adapter。
- 禁止直接调用 `HttpChatCompletionRepository` 或绕过 LLM API log/proxy/policy。

验收：

- Agent Mode off，现有生成完全不变。
- Agent Mode on，能完成一次 one-step generation 并提交为聊天消息。
- 同一稳定聊天的 regenerate/swipe 产生独立 `runId`，但共享由 `stableChatId` 派生的 `workspaceId`。
- `events.jsonl` 包含 run started、workspace initialized、context assembled、model completed、workspace write、checkpoint created、artifact assembled、run committed。
- `output/main.md` 可在 workspace 中读取。
- commit 后 message extra 包含 agent metadata。
- cancel 能让 run 进入 `Cancelled`，不提交半成品。
- 必需 artifact 缺失时 run fail-fast。
- Agent stream/timeline event 使用 `AgentRunEvent`，不复用 `ChatCompletionStreamEvent::Chunk` 作为语义事件。

风险：

- Commit 与 windowed payload 保存路径容易冲突。
- `PromptSnapshot` 结构过于 provider-specific。

缓解：

- Commit 必须通过既有 ChatService/保存队列契约。
- 文档和 DTO 明确 `PromptSnapshot` 是过渡输入，不是长期模型。

ROI：极高。  
风险：中低。

## Phase 2：最小工具循环

目标：从 one-step 进入真正 Agent loop，但工具范围保持克制。Phase 2 的重点不是一次性做完整工具平台，而是先建立可审计、可恢复、可扩展的工具循环飞轮。

设计原则：

- Agent tool loop 由 Rust runtime 拥有，不通过递归 `Generate()` 实现。
- 工具结果写入 run journal / workspace，不写入 chat message。
- 模型可见 schema 保持扁平、单操作、标量化，避免把核心参数设计成 `files[]`、`edits[]`、`hunks[]`。
- Phase 2 默认只支持 provider-native tool call；不支持时 fail-fast，不静默模拟。
- denied tool、非法路径、必需 artifact 缺失、native metadata 丢失都必须 fail-fast。
- workspace-mutating tool 只能通过 WorkspaceRepository 写入，并在成功后 checkpoint。

### Phase 2A：Agent Run Loop 地基

目标：把 Phase 1 one-shot 改造成最小多轮 loop，只开放足够验证飞轮的工具。

当前状态（截至 2026-04-26）：Phase 2A 后端工具循环与前端 dryRun adapter 已落地。它是最小可运行地基，不是完整 Agent 产品面。

工具：

```text
workspace.write_file(path: string, content: string)
workspace.finish(final_path?: string, reason?: string)
```

说明：文档中的 dotted name 是内部 canonical name。发送给 OpenAI-compatible function calling 时使用 provider-safe alias，例如 `workspace_write_file`、`workspace_finish`，runtime 再映射回 canonical name。

Phase 2A 当前只开放上述两个工具。`workspace.read_file`、`workspace.apply_patch`、`chat.search`、`skill.read`、MCP 工具均未进入当前 registry。多阶段 smoke 应通过多次 `workspace.write_file` 写 `plan/`、`scratch/`、`summaries/`、`output/` 来验证循环。

后端新增：

- `ToolSpec` / `ToolCall` / `ToolResult` domain model。
- 内建 tool registry，输出 OpenAI-compatible function tool schema。
- 最小 tool dispatcher，只负责 `workspace.write_file` 与 `workspace.finish`。
- provider-normalized tool call 提取器。
- tool result 作为 OpenAI-compatible `tool` message 回填下一轮请求。
- `workspace.finish` 结束 loop，并验证 `output/main.md` 等必需 artifact。
- 工具循环最大 6 轮；超轮次、模型不调用工具、finish 后继续调用工具都会 fail-fast。
- writable workspace path 当前限制为 `output/`、`scratch/`、`plan/`、`summaries/`。

前端新增：

- `api.agent.startRunFromLegacyGenerate()`：Phase 2A 推荐入口，内部调用 Legacy `Generate(..., dryRun = true)` 并监听 `GENERATE_AFTER_DATA` 捕获 prompt snapshot。
- `api.agent.startRunWithPromptSnapshot()`：低层入口，调用方显式传入 `promptSnapshot.chatCompletionPayload`。
- `api.agent.subscribe()`：当前为 polling wrapper，返回幂等 unsubscribe。
- `api.agent.readWorkspaceFile()`：读取 run workspace 的 UTF-8 文本文件。
- `api.agent.prepareCommit()` / `commit()` / `finalizeCommit()`：前端桥接 SillyTavern `saveReply()` 完成聊天写入。

兼容边界：

- Legacy `Generate(..., dryRun = true)` 不返回 payload；返回值是 `undefined`，payload 只通过事件暴露。
- Agent Mode off 时不改变 Legacy `Generate()`、ToolManager 或上游事件语义。
- Agent tools 由 Rust runtime 独占注册；Phase 2A 拒绝外部 `tools`、`tool_choice`、已有 `role: "tool"` 和已有 `tool_calls`。
- `stream: true` 与 `autoCommit: true` 当前显式拒绝。
- 不保留 `api.agent.startRun()` 旧 alias；公共启动方法名必须表达职责。

验收：

- run 可经历 `model -> tool -> model -> finish` 多轮循环。
- `workspace.write_file` 写入 workspace，不写 chat。
- 工具调用、结果、失败、耗时进入 journal。
- mutation 成功后创建 checkpoint。
- `workspace.finish` 后进入 artifact assembly 与 `AwaitingCommit`。
- 模型不调用工具、未知工具、工具超轮次、非法 path 均暴露明确错误。

### Phase 2B：Workspace 读改能力

目标：让 Agent 能读取、检查、精确修改 workspace 文件。

工具：

```text
workspace.list_files(path?: string, depth?: integer)
workspace.read_file(path: string, start_line?: integer, line_count?: integer)
workspace.apply_patch(path: string, old_string: string, new_string: string, replace_all?: boolean)
```

约束：

- `apply_patch` 使用 Claude Code 风格的单文件精确替换，不使用 JSON hunk 数组。
- mutating tool 维护 read-state / file version；未读、版本变化、匹配多处应返回可恢复 tool error。
- policy denied 与 path traversal 仍 fail-fast。

### Phase 2C：Chat/Skill/WorldInfo 只读上下文工具

目标：把高 ROI 的只读上下文接入 Agent，但不污染 SillyTavern chat schema。

候选工具：

```text
chat.search(query: string, limit?: integer, before_message_id?: string)
chat.read_history_tail(limit?: integer)
chat.read_history_before(message_id: string, limit?: integer)
skill.list()
skill.read(id: string)
worldinfo.read_activated()
```

约束：

- chat tools 只返回 snippet / stable ref，不返回可写句柄。
- skill/worldinfo 缺失时按工具语义返回 recoverable error 或 fail-fast，不静默空结果。
- 仍不触发 Legacy `TOOL_CALLS_*` 或 `GENERATION_*` 事件。

### Phase 2D：Provider 与策略硬化

目标：把工具循环从“可跑通”提升到“可长期维护”。

内容：

- provider schema sanitizer：canonical schema 深拷贝后按 OpenAI / Claude / Gemini / Responses 降级。
- profile 显式声明 `tool_call_mode = native | text_protocol | disabled`。
- tool budget、allowed tools、approval hook。
- 前端 timeline 展示 tool events；approval UI 仅在存在需要审批的工具后启用。
- 对 Claude/Gemini/Responses 的 tool call id、reasoning signature、native metadata 做回归测试。

风险：

- provider tool schema 差异。
- 工具错误被吞掉导致模型无法自我修复。
- 与 Legacy ToolManager 事件语义混淆。

缓解：

- 内部统一 `ToolCall` / `ToolResult`，provider 差异只在 adapter 层。
- recoverable tool error 作为 tool result 返回模型；policy/system failure fail-fast。
- Agent timeline 只使用 `AgentRunEvent`，最终 commit 才通过既有 chat save 边界。

ROI：高。  
风险：中。

## Phase 3：Agent Profile + Preset Agent Schema + Plan Policy

目标：把创作者自由度接入 runtime。

新增能力：

```text
AgentProfile
Preset agent schema
Prompt/Context policy
Tool policy
Budget policy
PlanPolicy
ProfileRouter
Strict / Free / Hybrid plan
```

Profile 最小字段：

```text
id
displayName
presetRef
modelRef
contextPolicy
visibleResourcePolicy
toolPolicy
planPolicy
summaryPolicy
switchPolicy
outputPolicy
budgetPolicy
```

Plan node 最小字段：

```text
id
title
locked
profileId
allowedTools
visibleFiles
maxRounds
contextBudget
expectedArtifacts
approvalRequired
```

验收：

- Preset 可以声明 Agent Profile。
- Strict plan 节点顺序和工具限制被 runtime 强制执行。
- Free plan 可以让模型创建/修改计划，但仍受全局 policy 限制。
- Hybrid plan 支持 locked 与 free 节点混合。
- profile switch 写 journal。
- plan policy violation fail-fast。

风险：

- 过早把 schema 做得过大。
- Preset 作者体验被复杂字段压垮。

缓解：

- 第一版 schema 保持最小稳定核心。
- UI 可以提供简单模式，高级字段保留给作者。

ROI：高。  
风险：中高。

## Phase 4：Backend Context Assembly 后移

目标：逐步减少对前端 dryRun PromptSnapshot 的依赖。

新增能力：

- Rust 侧 `GenerationIntent`。
- Rust 侧读取 chat history/windowed payload。
- Rust 侧读取 preset/character/world info/user profile。
- Typed `ContextFrame`。
- Prompt components 与宏。
- Context budget resolver。

过渡策略：

```text
Phase 1-3:
  PromptSnapshot + minimal ContextFrame

Phase 4:
  GenerationIntent + structured ContextFrame
  legacy PromptSnapshot only as compatibility fallback
```

验收：

- Agent 可在不复制完整 chat 的前提下读取历史。
- `ChatHistory`、`WorldInfo`、`WorkspaceFile`、`ToolResults` 等 component 可独立预算。
- Provider adapter 不知道 TauriTavern prompt component 的业务来源。
- Legacy Generate 仍不变。

风险：

- 世界书、扩展 prompt、PromptManager 语义复杂，容易破坏上游兼容。

缓解：

- 先迁移可独立验证的 component。
- 对世界书/扩展 prompt 保留 PromptSnapshot 兼容桥，直到测试足够。

ROI：中高。  
风险：高。

## Phase 5：MCP 独立集成

目标：实现 MCP 作为独立平台能力，再让 Agent 消费它。

新增 API：

```text
api.mcp.listServers
api.mcp.connectServer
api.mcp.listTools
api.mcp.callTool
api.mcp.listResources
api.mcp.readResource
api.mcp.listPrompts
api.mcp.getPrompt
```

后端新增：

- `McpClientService`
- `McpRepository`
- server config store
- per-server capability allowlist
- approval policy

安全底线：

- stdio command 必须来自用户设置或 allowlist。
- Agent/Preset/角色卡/世界书不能写 MCP command。
- 初期禁用或强审批 MCP Sampling。
- 危险 tool call 必须用户审批。

验收：

- 非 Agent 模式下可以列出和手动调用 MCP tool/resource/prompt。
- Agent 可以把 MCP tool 纳入 ToolRegistry。
- MCP resource 可作为 WorkspaceResource 或 ContextFrame component。
- denied MCP tool fail-fast。
- arbitrary stdio command 被拒绝。

ROI：中高。  
风险：高。

## Phase 6：Skill 系统与创作者资源包

目标：用渐进披露的纯文本 Skill 扩展 Agent 写作能力。

Skill 结构：

```text
skills/
  skill-name/
    SKILL.md
    examples/
    assets/
```

能力：

- `skill.list`
- `skill.read(name, section?)`
- Preset/角色卡/扩展声明可见 skill。
- Skill 可作为 virtual/materialized workspace resource。

验收：

- Agent 默认只看到 skill 摘要，不自动吞全文。
- profile/preset 可以控制 skill 可见性和预算。
- skill.read 结果写 journal。
- 缺失 skill fail-fast 或返回 recoverable tool error，取决于 tool call 语义。

ROI：中。  
风险：中。

## Phase 7：Extension Tool Provider / 多模态 / 图记忆

目标：在核心 runtime 稳定后开放生态扩展。

候选能力：

- extension 注册工具。
- frontend tool bridge。
- WASM/QuickJS sandbox 工具。
- 多模态 artifact。
- 图片/图记忆。
- graph memory search。

准入条件：

- Tool Registry/Policy/Journaling 已稳定。
- Approval UI 已稳定。
- Security tests 已覆盖 path、policy、MCP、extension boundary。

不建议在 Phase 1-4 提前做这些能力。

ROI：取决于生态。  
风险：中高到高。

## 持续验收清单

每个阶段合并前都必须回答：

- Agent Mode off 是否完全不影响 Legacy Generate？
- 新增 API 是否挂在 `window.__TAURITAVERN__.api.*`？
- 新增后端流程是否符合 Clean Architecture？
- 所有副作用是否都有 journal？
- workspace path 是否被统一校验？
- 工具结果是否没有进入 chat 楼层？
- commit 是否遵守 windowed payload 保存契约？
- policy violation 是否 fail-fast？
- 移动端内存/历史读取是否没有倒退？
- 测试是否覆盖本阶段新增 contract？

## 建议优先级

最推荐的近期顺序：

1. 完成 Phase 0。
2. 做 Phase 1，但 UI 只做最小 timeline。
3. 做 Phase 2 内置 workspace/chat/skill 工具。
4. 做 Phase 3 Profile/Plan。
5. 再决定 Phase 4 与 Phase 5 的先后。

如果团队资源有限，不要先做 MCP 或扩展工具。没有 Workspace + Journal + Commit 这三个地基，任何工具生态都会快速堆成难维护的临时补丁。
