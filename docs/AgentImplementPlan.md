# TauriTavern Agent Implementation Plan

本文档记录 **当前可继续开发的实施基线**，不再保留 Phase 0 / Phase 1 / Phase 2A 的展开式历史计划。旧阶段已经转化为当前架构约束；需要追溯背景时看 `docs/TauriTavernAgentDesignPlan.md`，需要看事实状态时以 `docs/CurrentState/AgentFramework.md` 为准。

## 1. 当前基线

截至 2026-04-26，Agent 已进入 Phase 2B 后的可运行状态：

- Rust 后端已拥有 Agent domain model、runtime、workspace、journal、checkpoint、commit bridge。
- 前端 Host ABI 已挂载 `window.__TAURITAVERN__.api.agent`。
- Agent 启动仍通过 `PromptSnapshot` 过渡输入；`GenerationIntent + ContextFrame` 尚未接管 context assembly。
- Legacy Generate 尚未默认切换到 Agent；Agent 目前通过控制台、扩展或后续 UI 显式调用。
- LLM 调用复用 `ChatCompletionService::generate_with_cancel()`，不得绕过现有 provider、secret、proxy、日志、endpoint policy、iOS policy 和取消链路。
- Tool loop 由 Rust runtime 独占推进，不递归调用前端 `Generate()`。

Phase 0 / 1 / 2A 的必要遗产只保留为这些不变量：

- Agent Mode off 时，上游 SillyTavern `Generate()`、ToolManager、事件顺序与 chat 保存语义不变。
- `stableChatId` 是长期聊天身份；`workspaceId` 由 `kind + stableChatId` 派生；`runId` 表示单次执行。
- 所有 run event 进入 append-only journal，不伪装成上游 `GENERATION_*` / `TOOL_CALLS_*` 事件。
- 工具结果进入 workspace / journal / 下一轮 model request，不写入 chat 楼层。
- 最终聊天写入由前端 commit bridge 走 SillyTavern `saveReply()`。
- `PromptSnapshot` 是兼容桥，不是长期上下文架构。

## 2. 当前 Host ABI

已落地入口：

```ts
api.agent.startRunFromLegacyGenerate(input?)
api.agent.startRunWithPromptSnapshot(input)
api.agent.subscribe(runId, handler, options?)
api.agent.cancel(runId)
api.agent.readEvents(input)
api.agent.readWorkspaceFile(input)
api.agent.prepareCommit(input)
api.agent.commit(input)
api.agent.finalizeCommit(input)
```

明确不存在公共 `api.agent.startRun()` alias。启动入口必须通过名称表达来源：

- `startRunFromLegacyGenerate()`：使用 Legacy `Generate(..., dryRun = true)` 捕获当前 prompt 语义。
- `startRunWithPromptSnapshot()`：调用方已经持有 `promptSnapshot.chatCompletionPayload`。

当前显式拒绝：

- `stream: true`
- `autoCommit: true`
- prompt snapshot 中已有 external `tools`
- external `tool_choice`
- 已有 `role: "tool"` 或 assistant `tool_calls`

当前 future API 只保留显式 throw：

```ts
approveToolCall()
listRuns()
readDiff()
rollback()
```

## 3. 当前工具集

内部 canonical name 使用 dotted form；发给 OpenAI-compatible function calling 时使用 provider-safe alias。

| Canonical name | Model alias | 类型 | 当前语义 |
| --- | --- | --- | --- |
| `workspace.list_files` | `workspace_list_files` | read-only | 列出模型可见 workspace 文件。`path` 省略、空字符串、`.`、`./` 都表示 workspace root。 |
| `workspace.read_file` | `workspace_read_file` | read-only | 读取 UTF-8 文本文件，返回行号；完整读取会记录 read-state。 |
| `workspace.write_file` | `workspace_write_file` | mutating | 写完整 UTF-8 文件；成功后记录 read-state 并创建 checkpoint。 |
| `workspace.apply_patch` | `workspace_apply_patch` | mutating | Claude Code 风格 `old_string` / `new_string` 精确替换；成功后创建 checkpoint。 |
| `workspace.finish` | `workspace_finish` | control | 结束 loop；默认 final artifact 是 `output/main.md`。 |

当前没有 `chat.search`、`skill.read`、WorldInfo 只读工具、MCP 工具、shell 工具或外部 extension tools。

Workspace 当前模型可见 / 可写根目录由 `WorkspaceAccessPolicy::phase2b_default()` 集中定义：

```text
output/
scratch/
plan/
summaries/
```

这个 root 集合是当前产品策略，不是路径安全边界。路径规范化、安全拒绝、symlink/escape 防护仍属于 host / repository 不变量。

## 4. 工具错误语义

必须区分两类错误：

- **Recoverable tool error**：模型参数、路径字符串、可见/可写策略、文件不存在、patch 未完整读取、sha 过期、匹配 0 次或多次等模型可修正问题。返回 `AgentToolResult { is_error: true }`，写入 `tool_call_failed` warn event，并作为 tool message 回填下一轮模型。
- **Fatal runtime error**：journal 写入失败、workspace repository 内部 IO 错误、manifest/checkpoint 损坏、模型响应结构不可解析、取消、序列化失败、状态机错误等宿主级问题。直接让 run 进入 failed 或 cancelled。

这个边界的目标是让模型能自我修正普通工具调用错误，同时不隐藏真实系统错误。

## 5. 当前运行流

```text
api.agent.startRunWithPromptSnapshot(input)
  ↓
前端解析 chatRef / stableChatId
  ↓
start_agent_run(dto)
  ↓
AgentRuntimeService::start_run()
  ↓
创建 AgentRun / workspaceId / run workspace
  ↓
initialize_run 写 manifest / prompt snapshot / workspace root
  ↓
prepare_agent_tool_request 注入 Rust-owned tools
  ↓
model -> tool -> model -> ... -> workspace.finish
  ↓
工具调用参数与结果写入 workspace refs
  ↓
workspace mutation 成功后 checkpoint
  ↓
validate_final_artifact(output/main.md)
  ↓
状态进入 awaiting_commit
  ↓
prepareCommit / saveReply / finalizeCommit
```

工具循环最多 80 轮。超过后以 `agent.max_tool_rounds_exceeded` 失败。模型直接输出文本且不调用工具会以 `model.tool_call_required_phase2b` 失败。

## 6. 下一步实施顺序

### 6.1 Phase 2C：上下文只读工具

目标：让 Agent 能安全读取对话、角色、世界书与创作者资源，而不是继续把所有上下文塞进一次 prompt snapshot。

优先候选：

```text
chat.read_history_tail(limit?: integer)
chat.search(query: string, limit?: integer)
worldinfo.read_activated()
skill.list()
skill.read(id: string)
```

约束：

- 先做只读工具，不给模型可写 chat 句柄。
- 返回 snippet / stable ref，避免巨大上下文内联。
- 不触发上游 `GENERATION_*` 或 `TOOL_CALLS_*` 事件。
- 缺失资源按 recoverable tool error 返回；宿主读取失败才 fatal。

### 6.2 Phase 2D：Provider 与策略硬化

目标：让工具循环从“OpenAI-compatible 可跑”变成“多 provider 可维护”。

内容：

- provider schema sanitizer：canonical schema 深拷贝后按 OpenAI / Claude / Gemini / Responses 降级。
- 保留 provider-native tool call metadata，尤其是 Claude/Gemini 的 tool id、reasoning signature 或等价字段。
- profile 显式声明 allowed tools、tool budget、tool call mode。
- unknown tool、schema mismatch、missing native metadata 必须有测试。

### 6.3 Phase 3：Timeline UI 与人工控制

目标：给创作者可理解、可暂停、可提交的 Agent run 体验。

内容：

- Agent Mode toggle 与主发送按钮分流。
- 最小 timeline/event viewer。
- workspace artifact viewer。
- tool error / recovery 状态展示。
- commit preview 与手动提交。
- cancel UI。

不做：

- 不把 Agent event 冒充 Legacy Generate event。
- 不在 UI 中直接编辑 run repository 文件。

### 6.4 Phase 4：Diff / Rollback / Resume

目标：让多轮创作具备可控回退能力。

内容：

- `readDiff()`：基于 checkpoint 对 workspace 文本文件生成 diff。
- `rollback()`：先只恢复 run workspace，不修改已提交聊天消息。
- `resumeRun()`：明确 run continuation 语义，避免复用已 closed run 的状态机。

### 6.5 Phase 5：MCP / Skill / Extension Tool

目标：引入外部工具生态，但保持 Tauri-native 安全边界。

约束：

- MCP Host ABI 独立于 Agent Mode。
- STDIO command/config 不得由 prompt、Preset、角色卡、世界书或第三方 JSON 任意写入。
- 危险工具必须进入 capability policy 与审批。
- Agent 消费 MCP tool 前必须经过 profile / policy resolution。

## 7. 验收命令

Agent 相关变更至少运行：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml agent --lib
cargo check --manifest-path src-tauri/Cargo.toml
git diff --check
```

涉及前端 Host ABI、类型或契约时再运行：

```bash
pnpm run check:types
pnpm run check:frontend
pnpm run check:contracts
```

## 8. 每次修改必须同步的文档

- 当前事实：`docs/CurrentState/AgentFramework.md`
- 架构边界：`docs/AgentArchitecture.md`
- 硬契约：`docs/AgentContract.md`
- Host ABI：`docs/API/Agent.md`、`docs/FrontendHostContract.md`、`src/types.d.ts`
- 工具语义：`docs/Agent/ToolSystem.md`
- workspace 语义：`docs/Agent/Workspace.md`
- 事件语义：`docs/Agent/RunEventJournal.md`

任何文档如果继续描述“当前只开放 `workspace.write_file` / `workspace.finish`”，都应视为过期并立即修正。
