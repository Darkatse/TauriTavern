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
startRun
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
- `window.__TAURITAVERN__.api.agent.startRun()` 先通过 `api.chat.open(chatRef).stableId()` 解析 `stableChatId`，再调用 Tauri command。
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

目标：从 one-step 进入真正 Agent loop，但工具范围保持克制。

内置工具：

```text
workspace.list_files
workspace.read_file
workspace.write_file
workspace.apply_patch
workspace.create_checkpoint
workspace.finish
chat.search
chat.read_history_tail
chat.read_history_before
skill.list
skill.read
worldinfo.read_activated
```

后端新增：

- `ToolSpec`
- `ToolCall`
- `ToolResult`
- `ToolRegistryService`
- `ToolDispatchService`
- `ToolPolicyResolver`
- tool event journal
- tool approval state

模型输出解释：

- 第一阶段可以支持 provider-native tool call。
- 对不支持 tool call 的 provider，不应静默模拟；应根据 profile 明确拒绝或使用受控文本协议，并在 profile 中声明。

验收：

- 模型请求工具时，run 进入 tool dispatch 流程。
- 工具参数、结果、错误、耗时写 journal。
- 工具结果不写入 chat message。
- 工具结果可作为 `ToolResults` component 进入下一轮模型请求。
- denied tool fail-fast，并记录 policy violation。
- workspace-mutating tool 后创建 checkpoint。
- `workspace.finish` 结束 loop 并进入 artifact assembly。

风险：

- provider tool schema 差异。
- 工具错误被吞掉导致模型无法自我修复。

缓解：

- 引入 provider-agnostic `ToolCall` / `ToolResult`。
- 错误分为 recoverable tool error 与 system failure。

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
