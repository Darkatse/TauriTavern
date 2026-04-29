# TauriTavern Agent Architecture

本文档定义 TauriTavern Agent 系统的总体架构。它不是功能清单，而是后续实现必须遵守的架构地图。

配套阅读顺序：

1. `docs/CurrentState/AgentFramework.md`：当前真实实现状态与手动 smoke。
2. `docs/AgentArchitecture.md`：系统边界、分层、数据流。
3. `docs/AgentContract.md`：不可破坏的不变量与 fail-fast 约束。
4. `docs/AgentImplementPlan.md`：分阶段实施计划与验收标准。
5. `docs/Agent/Workspace.md`：Workspace、Artifact、Checkpoint 的存储语义。
6. `docs/Agent/RunEventJournal.md`：Run Event、状态机、恢复/取消语义。
7. `docs/Agent/ProfilesAndPreset.md`：Preset、Agent Profile、Plan Policy。
8. `docs/Agent/ToolSystem.md`：Tool Registry、Tool Result、权限与审批。
9. `docs/Agent/LlmGateway.md`：provider-agnostic LLM gateway 与现有 ChatCompletionService 的复用边界。
10. `docs/Agent/McpSkill.md`：MCP 与 Skill 的边界。
11. `docs/API/Agent.md`：前端 Host ABI 与 Tauri command 草案。
12. `docs/API/MCP.md`：非 Agent 模式下的 MCP Host ABI 草案。
13. `docs/Agent/TestingStrategy.md`：测试矩阵与回归守护。

## 1. 核心定义

Agent Mode 的核心定义是：

> 一次生成不是 LLM 返回一段字符串，而是 Agent 对一个受策略约束的 Workspace 进行一组可审计、可回滚的编辑，最后由运行时把 Artifact 组装并提交为聊天消息。

因此 Agent 不是更复杂的 `Generate()`，也不是 SillyTavern 工具调用递归的后端移植版。Agent 是一条新的生成路径：

```text
Legacy Generate
  保持 SillyTavern 1.16.0 的事件语义、扩展兼容和 one-shot 体验

Agent Generate
  Rust-owned Workspace Runtime + Profile + Plan + Tools + Journal + Checkpoint

MCP / Tool Direct Call
  非 Agent 模式下也可显式调用 MCP 或工具，但不拥有 Agent Run
```

三条路径可以共享 LLM gateway、tool registry、MCP client、chat repository、windowed payload 和 tokenization service，但不能互相污染。

## 2. Ground of Truth

本架构以当前仓库代码和文档为准：

- 后端已经采用 Clean Architecture，依赖方向是外层依赖内层、内层定义接口、外层实现接口。见 `docs/BackendStructure.md:7`、`docs/BackendStructure.md:40`。
- 应用服务由 `AppState` 管理并在 `bootstrap::build_services()` 装配。见 `src-tauri/src/app.rs:36`、`src-tauri/src/app/bootstrap.rs:150`。
- 当前 LLM 请求经过 `ChatCompletionService`，该服务负责 provider 解析、iOS policy、endpoint override policy、payload build、prompt caching 和取消注册。见 `src-tauri/src/application/services/chat_completion_service/mod.rs:32`、`src-tauri/src/application/services/chat_completion_service/mod.rs:302`、`src-tauri/src/application/services/chat_completion_service/mod.rs:358`。
- 当前 LLM API 日志依赖 bootstrap 中装配的 `LoggingChatCompletionRepository` wrapper；Agent 不得直接调用 `HttpChatCompletionRepository` 绕过日志、proxy、secret 或 policy。见 `src-tauri/src/app/bootstrap.rs:372`。
- 当前 chat payload 分片读写由 `ChatService` 和 `ChatRepository` 承担，windowed save/patch 是正式契约。见 `src-tauri/src/application/services/chat_service.rs:495`、`src-tauri/src/application/services/chat_service.rs:563`、`src-tauri/src/domain/repositories/chat_repository.rs:145`、`src-tauri/src/domain/repositories/chat_repository.rs:162`。
- 前端 Public ABI 的统一入口是 `window.__TAURITAVERN__`，应保持小而稳定。见 `docs/FrontendHostContract.md:92`、`src/tauri/main/bootstrap.js:139`。
- 当前 `Generate()` 支持 dryRun，并在 `GENERATE_AFTER_DATA` 提供生成请求数据。见 `src/script.js:4660`、`src/script.js:5743`。
- 当前 SillyTavern 工具调用会递归回 `Generate()`，并把工具调用结果保存成 `is_system` chat message。这是 Agent 必须摆脱的旧结构。见 `src/script.js:5826`、`src/script.js:5847`、`src/script.js:5959`、`src/scripts/tool-calling.js:868`、`src/scripts/tool-calling.js:887`。

## 3. 设计目标

Agent 系统必须同时满足五个目标：

1. 兼容：Agent Mode off 时，Legacy Generate 的行为、事件、扩展注入、世界书语义不变。
2. 可维护：Agent runtime 位于 Rust application layer，流程可测试，基础设施通过 trait 注入。
3. 可审计：所有 LLM call、tool call、file write、checkpoint、profile switch、commit 都进入 append-only journal。
4. 可回滚：Agent 修改 workspace，不直接写 chat；commit 后仍可根据 artifact/checkpoint 回滚聊天消息。
5. 创作者自由：Preset/角色卡/扩展/Skill 能控制可见内容、工具、预算、计划模式和输出结构，但不能突破宿主安全边界。

## 4. 非目标

第一阶段不追求这些事项：

- 不重写完整 SillyTavern PromptManager。
- 不把所有历史消息复制到 workspace。
- 不把工具结果继续保存为 chat 楼层。
- 不引入 shell 工具作为默认能力。
- 不把 MCP 当作 Agent Runtime 本体。
- 不执行世界书/角色卡/扩展提供的任意后端脚本。
- 不把多个 Agent 过早实体化为互相争夺最终回复的独立运行时。

## 5. 总体数据流

短期过渡流：

```text
用户发送消息
  ↓
前端保持旧 Generate 前置语义
  ↓
window.__TAURITAVERN__.api.agent.startRunFromLegacyGenerate()
  ↓
adapter 调用 Generate(type, { ...options, agentMode: true }, dryRun = true)
  ↓
GENERATE_AFTER_DATA 事件暴露 generate_data
  ↓
adapter 构造 promptSnapshot.chatCompletionPayload
  ↓
解析 stableChatId
  ↓
Rust AgentRunService
  ↓
Workspace 初始化
  ↓
ContextFrame 初步组装
  ↓
Agent Run State Machine
  ├─ model call
  ├─ tool call
  ├─ workspace patch
  ├─ checkpoint
  ├─ optional plan/profile switch/summary
  └─ finish
  ↓
ArtifactAssembler
  ↓
Committer 通过 chat service/windowed payload 保存
  ↓
聊天 UI 展示最终消息，timeline 展示 Agent 过程
```

中长期目标流：

```text
GenerationIntent
  ↓
Rust ContextAssemblyService
  ├─ chat history windowed read/search
  ├─ world info activation result
  ├─ preset/character/user profile
  ├─ workspace files
  ├─ skill snippets
  └─ tool definitions/results
  ↓
Provider-agnostic ModelRequest
  ↓
LLM Gateway / provider adapter
```

短期接受 `PromptSnapshot` 是兼容策略，不是最终架构。任何实现都必须让后续从 `PromptSnapshot` 迁移到结构化 `GenerationIntent + ContextFrame` 成为增量演进，而不是重写。

注意：`Generate(..., dryRun = true)` 不是纯函数。它会触发上游事件、执行 prompt assembly、触发部分 world info 扫描与工具注册判断；它只是跳过实际模型请求、聊天写入和工具执行等关键副作用。Agent 文档和代码中都不能把 dryRun 描述为“无副作用”。

还要注意：Legacy `Generate(..., dryRun = true)` 不返回 payload；它在 `GENERATE_AFTER_DATA` 事件中暴露 `generate_data`，然后 resolve `undefined`。当前前端 adapter 必须监听事件捕获 payload，调用方不应依赖 `await Generate(..., true)` 的返回值。

### 5.1 当前落地边界

截至 2026-04-26，当前已落地的是 Phase 2B workspace 读改工具循环，而不是完整 Agent 产品面：

- Public Host ABI 入口为 `api.agent.startRunFromLegacyGenerate()` 与 `api.agent.startRunWithPromptSnapshot()`，没有 `startRun()` alias。
- `startRunFromLegacyGenerate()` 是当前推荐的兼容桥；它捕获 Legacy prompt 语义，同时禁用 Legacy ToolManager tools。
- `startRunWithPromptSnapshot()` 是低层测试/集成入口；调用方必须提供不含 external tools/tool turns 的 chat completion payload。
- 后端当前开放 `workspace.list_files`、`workspace.read_file`、`workspace.write_file`、`workspace.apply_patch`、`workspace.finish` 五个内建工具，对模型暴露为 provider-safe alias。
- 当前模型可见 / 可写 workspace 根为 `output/`、`scratch/`、`plan/`、`summaries/`；`input/`、`tool-args/`、`tool-results/`、`checkpoints/` 与 `events.jsonl` 不作为模型工具资源暴露。
- 工具循环最多 80 轮，必须以 `workspace.finish` 结束；模型直接输出文本会 fail-fast。
- 模型可修正的工具错误以 `is_error = true` tool result 回填下一轮；宿主级 IO、journal、checkpoint、序列化、取消和模型响应结构错误仍 fail-fast。
- `readDiff`、`rollback`、`resume-run`、tool approval、profile routing、MCP、timeline UI、主发送按钮 Agent toggle 仍未实现。

### 5.2 Run 与 Workspace 身份

Agent 身份必须分层：

```text
stableChatId  聊天长期身份，由 Host Chat API 解析
workspaceId   对话级 workspace 身份，由 kind + stableChatId 派生
runId         单次 Agent 执行身份，由 runtime 生成 UUID
chatRef       当前可定位引用，用于读取/commit guard
```

`workspaceId` 不能由完整 `chatRef` hash 决定。`chatRef` 中的文件名、角色名和当前打开状态可能变化；`stableChatId` 才是跨重命名、导入后修复和长期清理策略可依赖的身份。swipe/regenerate 应创建新的 `runId`，但仍落在同一个稳定聊天的 chat workspace 下。

## 6. 后端分层

建议模块边界：

```text
src-tauri/src/
  domain/
    models/agent/
      run.rs
      event.rs
      workspace.rs
      artifact.rs
      checkpoint.rs
      profile.rs
      plan.rs
      policy.rs
      tool.rs
      model.rs
    repositories/
      agent_run_repository.rs
      workspace_repository.rs
      checkpoint_repository.rs
      skill_repository.rs
      mcp_repository.rs

  application/
    dto/
      agent_dto.rs
      mcp_dto.rs
    services/
      agent_runtime/
        mod.rs
        run_state_machine.rs
        context_assembly.rs
        artifact_assembly.rs
        commit.rs
        profile_router.rs
        plan.rs
      workspace_service/
      tool_registry_service/
      tool_dispatch_service/
      llm_gateway_service/
      mcp_client_service/
      skill_service/

  infrastructure/
    repositories/
      file_agent_run_repository/
      file_workspace_repository/
      file_checkpoint_repository/
      file_skill_repository/
    apis/
      mcp/
    diff/

  presentation/
    commands/
      agent_commands.rs
      mcp_commands.rs
```

关键规则：

- `presentation` 只做 DTO 校验、权限/通道参数拆解、调用 application service、错误映射。
- `application/services/agent_runtime` 是编排中心，但它不直接操作文件系统、不直接发 HTTP、不直接管理 MCP subprocess。
- `domain` 定义纯模型与 repository/tool/gateway trait，不依赖 Tauri、tokio process、WebView、HTTP client。
- `infrastructure` 实现文件存储、MCP client、diff、外部 API 适配。
- 新服务必须在 `bootstrap::build_services()` 装配，并挂入 `AppState`，与现有服务生命周期一致。

## 7. 运行时核心组件

### 7.1 AgentRunService

职责：

- 创建 run。
- 驱动状态机。
- 追加 journal event。
- 调用 workspace、context、tool、LLM、checkpoint、artifact、commit 服务。
- 管理 cancel / await approval / resume。

`AgentRunService` 不应该知道具体文件路径如何落盘，也不应该直接拼 provider-specific payload。

### 7.2 WorkspaceService

职责：

- 创建对话级 workspace root 与 run workspace。
- 管理 materialized file、virtual resource、generated artifact。
- 应用 patch/write。
- 做 path normalization 与 traversal 拒绝。
- 生成 checkpoint snapshot/diff 所需文件视图。

详见 `docs/Agent/Workspace.md`。

### 7.3 ContextAssemblyService

职责：

- 把 chat history、world info、preset、character、user profile、workspace、skill、tool definitions、tool results、plan、diff summary 组织为 typed components。
- 根据 profile/preset policy 决定可见性、顺序和 token budget。
- 输出 provider-agnostic `ContextFrame` 或 `ModelRequestDraft`。

它是 SillyTavern prompt 灵活性的 Agent 版本，不能退化成硬编码 system/user 字符串。

### 7.4 LlmGatewayService

职责：

- 接受 provider-agnostic `ModelRequest`。
- 复用现有 provider 能力、policy 检查、prompt caching、logging、proxy/client 配置、cancellation。
- 输出 provider-agnostic `ModelResponse` / streaming delta / tool call。

第一期必须通过适配现有 `ChatCompletionService` 完成，而不是直接调用 `HttpChatCompletionRepository` 或新建 HTTP client。长期目标是在不破坏现有 chat completion API 的前提下抽出更明确的 model gateway。

当前 `ChatCompletionStreamEvent::Chunk` 只是 provider SSE `data` 字符串的桥接，不是 Agent timeline 语义事件。Agent 必须定义自己的 `AgentRunEvent`，不能把 provider stream chunk 当作 run event。

### 7.5 ToolRegistryService / ToolDispatchService

职责：

- 注册内置工具、MCP 工具、未来 extension bridge 工具。
- 根据 profile/preset/plan/run policy 解析可见工具与审批要求。
- 派发 tool call。
- 把 tool result 写入 journal 与 context store，不写入 chat message。

详见 `docs/Agent/ToolSystem.md`。

### 7.6 ArtifactAssemblyService / Committer

职责：

- 根据 workspace manifest 读取一个或多个 artifact。
- 组装 message body 与 message extra。
- 通过现有 chat 保存路径提交。
- commit 后写入 `agentRunId`、`agentCheckpointId`、`agentArtifacts` 等 metadata。

Commit 必须遵守 windowed payload 与保存串行化契约，不能直接写 JSONL 文件。

如果第一期由前端完成 commit，必须复用现有 `enqueueChatSave()` 串行化路径。如果后续改为后端直接 commit，后端必须提供等价的 per-chat 串行化、cursor/CAS/resync 语义，并仍通过 ChatService/GroupChatService 的正式保存边界。

## 8. 前端边界

Agent 前端能力必须挂在：

```js
window.__TAURITAVERN__.api.agent
```

MCP 直连能力必须挂在：

```js
window.__TAURITAVERN__.api.mcp
```

禁止新增散落的 `window.__TAURITAVERN_AGENT_*` 全局函数，除非是临时调试且不进入 Public Contract。

前端职责：

- 提供 Agent Mode 开关与最小 timeline UI。
- 在短期通过 `Generate(..., dryRun = true)` 生成 `PromptSnapshot`。
- 调用 `api.agent.startRunFromLegacyGenerate()` 或 `api.agent.startRunWithPromptSnapshot()`、订阅 event、展示状态、发起 cancel/approve/rollback/commit。
- Agent Mode off 时不改变 Legacy Generate。

前端不拥有 Agent 状态机，不递归调用 `Generate()` 实现工具循环，不把工具结果写进 chat 楼层。

Agent 自己的 run/timeline/tool events 必须通过 `api.agent.subscribe(...)` 或等价 Host ABI 通道暴露。除非实际执行 Legacy Generate 兼容路径，否则不得伪装成 `GENERATION_STARTED`、`TOOL_CALLS_PERFORMED` 等 SillyTavern 上游事件。

## 9. 状态机

最小状态：

```text
Created
  ↓
InitializingWorkspace
  ↓
AssemblingContext
  ↓
Planning? 
  ↓
Running
  ├─ CallingModel
  ├─ AwaitingToolApproval
  ├─ DispatchingTool
  ├─ ApplyingWorkspacePatch
  ├─ CreatingCheckpoint
  ├─ Summarizing?
  └─ SwitchingProfile?
  ↓
AssemblingArtifacts
  ↓
Committing
  ↓
Completed
```

终止状态：

```text
Completed
Cancelled
Failed
```

状态变化必须以 journal event 记录。UI 的实时事件可以从 journal 派生，但不能成为唯一真相源。

## 10. 兼容策略

### 10.1 Legacy Generate 不变

Agent Mode off 时：

- `Generate()` 入口、事件顺序、dryRun 语义、工具调用旧行为、自动继续、swipe/regenerate/impersonate/quiet 等行为不应改变。
- 第三方扩展依赖的事件仍按旧语义触发。
- `window.__TAURITAVERN__.api.chat` 不因 Agent 引入而改变。

### 10.2 Agent Mode 使用干净旁路

Agent Mode on 时：

- 可以复用 Legacy Generate 的前置 prompt assembly 作为过渡输入。
- 不应复用 Legacy ToolManager 的“工具结果保存成系统楼层”语义。
- 不应让 Agent tool loop 通过递归 `Generate()` 驱动。

### 10.3 上游语义保留

SillyTavern 上游的事件和 chat message 结构仍是兼容层的基础。Agent metadata 必须放在 `extra` 的 TauriTavern namespace 下，新增字段必须向后兼容，不能让旧 UI/扩展读取消息时崩溃。

## 11. 性能策略

- Chat history 默认是 virtual resource，通过 Rust chat repository/windowed payload 按需读取。
- Workspace 不复制完整历史、完整世界书、完整记忆库。
- Context assembly 必须有 token/resource budget。
- Tool result 进入 journal 后，进入 prompt 前可以摘要、裁剪或按需读取。
- 移动端默认更小预算、更短 checkpoint retention、更保守的并发。
- Streaming event 应该可节流，journal 应该可 append-only 顺序写入。

## 12. 安全策略

- Workspace path 必须相对 root 规范化，拒绝 `..`、绝对路径、symlink escape。
- MCP stdio server 只能来自用户显式配置或 allowlist，Agent/Preset/角色卡/世界书不能写 command。
- 危险工具必须审批，审批请求必须记录 journal。
- Hidden/private resource 不能被 context assembly 读入模型请求。
- Provider endpoint override 必须继续遵守现有 iOS policy 与 settings policy。
- 任何 policy violation 都必须显式进入 journal：若属于模型可修正的工具参数/权限问题，返回 recoverable tool error；若属于宿主安全或状态机问题，则 fail-fast。禁止静默降级为“工具不可见但继续跑”之类的模糊行为。

## 13. 最小 MVP

第一个可合并 Agent MVP 应只包含：

1. `api.agent.startRunWithPromptSnapshot()`。
2. run workspace。
3. `events.jsonl`。
4. `output/main.md`。
5. checkpoint snapshot。
6. 单次 LLM call。
7. artifact commit 到 chat。
8. 最小 timeline UI。
9. Agent Mode off 行为完全不变。

这个 MVP 的价值不是炫技，而是验证 runtime boundary、journal、workspace、commit、rollback 这些后续能力赖以存在的骨架。
