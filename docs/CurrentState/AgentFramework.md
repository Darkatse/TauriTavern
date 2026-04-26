# Agent Framework 当前进度

本文档用于后续记录 Agent 框架的实时开发进度。它不是架构设计源文档，也不替代 `docs/AgentArchitecture.md`、`docs/AgentContract.md`、`docs/AgentImplementPlan.md` 与 `docs/Agent/` 下的细节设计。

## 当前状态

截至 2026-04-26：

- Phase 2A Agent Runtime 工具循环地基已落地。
- `window.__TAURITAVERN__.api.agent` 已挂载最小可用 Host ABI。
- `window.__TAURITAVERN__.api.mcp` 尚未实现。
- 已完成第一轮架构/契约/实施计划/API 草案文档整理。
- Agent 细节文档已收拢到 `docs/Agent/`。
- Legacy Generate 尚未切换到 Agent 路径；当前 Agent 可通过 `api.agent.startRunFromLegacyGenerate()` 使用 dryRun adapter 启动，或通过 `api.agent.startRunWithPromptSnapshot()` 传入手工 prompt snapshot 测试。

Phase 2A 当前能力是“可审计最小工具循环 Agent run”：

- 前端提供 `api.agent.startRunFromLegacyGenerate()`、`startRunWithPromptSnapshot()`、`cancel()`、`readEvents()`、`readWorkspaceFile()`、`subscribe()`、`prepareCommit()`、`finalizeCommit()`、`commit()`。
- `startRunFromLegacyGenerate()` 使用 Legacy `Generate(..., dryRun = true)` 捕获当前 SillyTavern prompt 语义，构造 `promptSnapshot.chatCompletionPayload`，并禁用 Legacy ToolManager tools。
- `startRunWithPromptSnapshot()` 会在前端解析 `stableChatId`，后端只接受非空稳定聊天身份。
- 后端为每次执行生成独立 `runId`，并由 `kind + stableChatId` 派生稳定 `workspaceId`。
- 后端复用现有 `ChatCompletionService` 调用 LLM，不直接绕过 provider 解析、secret、proxy、日志、iOS policy、endpoint policy 或取消注册。
- Runtime 会初始化 workspace、保存 prompt snapshot、写入 append-only run event、执行 `model -> tool -> model -> finish` 循环，写出 `output/main.md`、创建 checkpoint，并进入 `awaiting_commit`。
- commit 目前由前端桥接：`prepare_agent_run_commit` 生成 draft，前端调用 SillyTavern `saveReply()` 写入当前聊天，再调用 `finalize_agent_run_commit` 完成 run。
- Phase 2A 明确拒绝 stream、autoCommit、external tools/tool turns；diff、rollback、resume、profile routing 仍是后续阶段。

Phase 2A 当前工具集非常克制：

| Canonical name | Model-facing alias | 状态 | 说明 |
| --- | --- | --- | --- |
| `workspace.write_file` | `workspace_write_file` | 已落地 | 写 UTF-8 文本到 run workspace；当前可写前缀为 `output/`、`scratch/`、`plan/`、`summaries/` |
| `workspace.finish` | `workspace_finish` | 已落地 | 结束工具循环；默认 final artifact 是 `output/main.md` |

当前没有 `workspace.read_file`、`workspace.apply_patch`、`chat.search`、`skill.read` 或 MCP 工具。多步骤测试应通过多次 `workspace_write_file` 产出阶段文件来验证工具循环，而不是要求模型读取 workspace 文件。

重要边界：

- Agent Mode off 时 Legacy `Generate()` 行为不变。
- `Generate(..., dryRun = true)` 本身不返回 prompt payload；它通过 `GENERATE_AFTER_DATA` 事件暴露 `generate_data`，然后 resolve `undefined`。Agent adapter 负责监听该事件并构造 `promptSnapshot.chatCompletionPayload`。
- 不存在公共 `api.agent.startRun()` alias。Phase 2A 公共启动入口只有 `startRunFromLegacyGenerate()` 与 `startRunWithPromptSnapshot()`。
- Agent timeline/event 不伪装为 SillyTavern `GENERATION_*` 或 `TOOL_CALLS_*` 事件。
- 当前没有 Agent mode toggle、最小 timeline UI 或主发送按钮接入。
- 当前没有 `GenerationIntent + ContextFrame` 原生组装；Phase 2A 使用 `promptSnapshot.chatCompletionPayload` 作为过渡输入。
- 当前没有 MCP Host ABI；MCP 仍是 Phase 5 目标。
- 当前 Agent ABI 的 `AgentChatRef` 在边界只接受/输出 `characterId`、`fileName`、`chatId`；内部 snake_case 字段名进入 ABI 会 fail-fast。

## 文档入口

- 高层架构：`docs/AgentArchitecture.md`
- 硬契约：`docs/AgentContract.md`
- 实施计划：`docs/AgentImplementPlan.md`
- 细节文档目录：`docs/Agent/README.md`
- Agent API 草案：`docs/API/Agent.md`
- MCP API 草案：`docs/API/MCP.md`

## 进度台账

| 日期 | Phase | 状态 | 变更/PR | 备注 |
| --- | --- | --- | --- | --- |
| 2026-04-26 | Phase 0 | 规划完成 | 文档整理 | 建立 Agent 架构、契约、实施计划、API 草案与细节文档目录 |
| 2026-04-26 | Phase 1 | 骨架落地 | 本地工作区 | 建立 Agent domain model、repository trait、文件仓库、runtime service、Tauri commands、前端 Host ABI 与 commit 桥接 |
| 2026-04-26 | Phase 2A | 前后端地基落地 | 本地工作区 | 后端最小工具循环与前端 dryRun prompt snapshot adapter 接入 |

## 实施检查表

| 项目 | 状态 | 代码入口 | 测试/验证 | 备注 |
| --- | --- | --- | --- | --- |
| Agent domain models | 已落地 | `src-tauri/src/domain/models/agent/mod.rs` | `cargo test --manifest-path src-tauri/Cargo.toml agent --lib` | `AgentRun` / `AgentRunEvent` / `WorkspacePath` / manifest / artifact / checkpoint；ABI 使用 camelCase |
| Repository traits | 已落地 | `src-tauri/src/domain/repositories/agent_run_repository.rs`、`workspace_repository.rs`、`checkpoint_repository.rs` | `cargo check --manifest-path src-tauri/Cargo.toml` | application layer 依赖 trait，不直接依赖文件系统 |
| FileAgentRepository | 已落地 | `src-tauri/src/infrastructure/repositories/file_agent_repository.rs` | `repository_round_trips_run_workspace_event_and_checkpoint` | 根目录为 `_tauritavern/agent-workspaces`；保存 run index、manifest、JSONL event、workspace file、checkpoint |
| AgentRuntimeService | 已落地 | `src-tauri/src/application/services/agent_runtime_service.rs`、`src-tauri/src/application/services/agent_runtime_service/` | `cargo test --manifest-path src-tauri/Cargo.toml agent --lib` | Phase 2A 已拆分 lifecycle/executor/loop/tool execution 等子模块 |
| LLM gateway wrapper | 部分落地 | `AgentRuntimeService` 复用 `ChatCompletionService::generate_with_cancel` | `cargo check --manifest-path src-tauri/Cargo.toml` | 还没有 provider-agnostic `ModelRequest`/`LlmGatewayService` 抽象 |
| Tauri commands | 已落地 | `src-tauri/src/presentation/commands/agent_commands.rs`、`registry.rs` | `cargo check --manifest-path src-tauri/Cargo.toml` | `start/cancel/readEvents/readWorkspaceFile/prepareCommit/finalizeCommit` |
| `api.agent` Host ABI | 已落地 | `src/tauri/main/api/agent.js`、`src/tauri/main/api/agent-prompt-snapshot.js`、`src/tauri/main/bootstrap.js`、`src/types.d.ts` | `pnpm run check:types`、`pnpm run check:frontend` | `startRunFromLegacyGenerate()` 为 Phase 2A dryRun adapter；`subscribe()` 当前为 polling；future API 显式 throw |
| Commit bridge | 已落地 | `src/tauri/main/api/agent.js` | 手动控制台测试路径 | 前端 `saveReply()` 写 chat，合并 `tauritavern.agent` metadata，再 finalize |
| Stable chat identity | 已落地 | `active-chat-ref.js`、`chat.js`、`agent.js`、`AgentRuntimeService` | `workspace_id_uses_stable_chat_id_not_character_chat_file_name` | `workspaceId = chat_ + sha256(kind + stableChatId)[0..16]` |
| 最小 timeline UI | 未开始 | - | - | 不伪装成 SillyTavern `GENERATION_*` 事件 |
| ToolRegistry/ToolDispatch | Phase 2A 已落地 | `src-tauri/src/application/services/agent_tools.rs`、`src-tauri/src/application/services/agent_tools/`、`agent_runtime_service/tool_execution.rs` | `cargo test --manifest-path src-tauri/Cargo.toml agent --lib` | 当前只开放 `workspace.write_file` 与 `workspace.finish` |
| `api.mcp` Host ABI | 未开始 | - | - | Phase 5，MCP 独立于 Agent Mode |

## 当前后端运行流

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
写入 manifest、prompt snapshot、run_created event
  ↓
调用 ChatCompletionService::generate_with_cancel()
  ↓
解析 provider-native tool_calls
  ↓
dispatch workspace.write_file / workspace.finish
  ↓
tool result 回填下一轮 request，直到 finish
  ↓
workspace.write_file 写入 output/main.md
  ↓
创建 checkpoint
  ↓
状态进入 awaiting_commit
  ↓
prepareCommit / commit / finalizeCommit
```

当前 run event 主要包括：

```text
run_created
generation_intent_recorded
status_changed
workspace_initialized
context_assembled
model_request_created
model_completed
tool_call_requested
tool_call_started
tool_result_stored
tool_call_completed / tool_call_failed
workspace_file_written
checkpoint_created
agent_loop_finished
artifact_assembled
commit_started
commit_draft_created
run_committed
run_completed
run_cancel_requested
run_cancelled
run_failed
```

工具循环最多 6 轮；超过后会以 `agent.max_tool_rounds_exceeded` 失败。Phase 2A 要求模型必须调用工具；如果模型直接输出文本且不调用工具，会以 `model.tool_call_required_phase2a` 失败。

## 当前文件布局

Agent workspace 由 `FileAgentRepository` 管理，当前物理布局：

```text
_tauritavern/agent-workspaces/
  index/
    runs/
      <run-id>.json
  chats/
    <workspace-id>/
      runs/
        <run-id>/
          manifest.json
          events.jsonl
          input/
            prompt_snapshot.json
          output/
            main.md
          tool-results/
            <tool-call-id>.json
          checkpoints/
            <checkpoint-id>.json
```

Workspace path 必须是相对路径；空路径、绝对路径、Windows drive prefix、NUL、`..` 会直接拒绝。

## 当前手动测试入口

目前没有 UI toggle。需要在前端控制台中调用：

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);

const agent = window.__TAURITAVERN__.api.agent;

const run = await agent.startRunFromLegacyGenerate({
  generationType: 'normal',
  options: { stream: false, autoCommit: false },
});

const stop = agent.subscribe(run.runId, event => console.log(event));
```

`startRunFromLegacyGenerate()` 会复用当前 SillyTavern Chat Completion prompt/settings 生成 `promptSnapshot`。需要手工构造 smoke payload 时仍可直接调用 `startRunWithPromptSnapshot()`，但不要注入 `tools`、`tool_choice` 或已有 tool turns。

### 多阶段工具循环 smoke

用于验证多轮工具回填、workspace 写入、checkpoint 与最终 artifact：

```js
(async () => {
  const tt = window.__TAURITAVERN__;
  await tt.ready;

  const agent = tt.api.agent;
  const openai = await import('/scripts/openai.js');
  const model = openai.getChatCompletionModel(openai.oai_settings);

  const messages = [{
    role: 'user',
    content: [
      'This is a TauriTavern Agent Phase 2A multi-step tool loop smoke test.',
      '',
      'You must complete the task using workspace tools only.',
      'Do not answer directly in chat text.',
      'Call exactly one tool per assistant turn. Wait for the tool result before the next step.',
      '',
      'Required workflow:',
      '1. Call workspace_write_file to write an outline to plan/outline.md.',
      '2. Call workspace_write_file to write a complete draft to scratch/draft.md.',
      '3. Call workspace_write_file to write concise revision notes to summaries/revision_notes.md.',
      '4. Call workspace_write_file to write the polished final answer to output/main.md.',
      '5. Call workspace_finish with final_path set to output/main.md.',
      '',
      'The final answer topic is:',
      'Explain why an Agent loop is better than one-shot generation for a Tauri-native SillyTavern fork.',
      '',
      'The final output must be polished Chinese prose, about 300-500 Chinese characters.'
    ].join('\n')
  }];

  const { generate_data: payload } = await openai.createGenerationParameters(
    openai.oai_settings,
    model,
    'normal',
    messages,
    { agentMode: true },
  );

  const run = await agent.startRunWithPromptSnapshot({
    chatRef: tt.api.chat.current.ref(),
    generationType: 'normal',
    promptSnapshot: { chatCompletionPayload: payload },
    options: { stream: false, autoCommit: false },
  });

  console.log('[agent run]', run);

  window.__agentRun = run;
  window.__agentStop?.();
  window.__agentStop = agent.subscribe(run.runId, e => {
    console.log('[agent]', e.seq, e.type, e.payload);
  });
})().catch(console.error);
```

检查阶段文件：

```js
await window.__TAURITAVERN__.api.agent.readWorkspaceFile({
  runId: window.__agentRun.runId,
  path: 'plan/outline.md',
});

await window.__TAURITAVERN__.api.agent.readWorkspaceFile({
  runId: window.__agentRun.runId,
  path: 'scratch/draft.md',
});

await window.__TAURITAVERN__.api.agent.readWorkspaceFile({
  runId: window.__agentRun.runId,
  path: 'summaries/revision_notes.md',
});

await window.__TAURITAVERN__.api.agent.readWorkspaceFile({
  runId: window.__agentRun.runId,
  path: 'output/main.md',
});
```

## 最近验证命令

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml agent --lib
cargo check --manifest-path src-tauri/Cargo.toml
pnpm run check:types
pnpm run check:contracts
pnpm run check:frontend
```

最近一次 Rust 侧验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml agent --lib`：10 passed。
- `cargo check --manifest-path src-tauri/Cargo.toml`：passed。

前端、类型与契约检查在 Phase 2A adapter 接入后通过；后续改动 `src/tauri/main/api/agent.js`、`src/tauri/main/api/agent-prompt-snapshot.js`、`src/types.d.ts` 或 docs contract 时必须重新运行。

## 已知待办

- 将 Agent run 接入可控 UI，而不是只靠控制台调用。
- 设计 Agent Mode toggle 与 Legacy Generate 的清晰分流，不改变 Agent Mode off 语义。
- 建立最小 timeline/event viewer。
- 将 `promptSnapshot` 过渡输入逐步替换为 `GenerationIntent + ContextFrame`。
- 拆出 provider-agnostic LLM gateway/model request，继续保留 `ChatCompletionService` 的既有策略链。
- 扩展 Phase 2B/2C 工具能力，继续确保工具结果留在 workspace/journal，不写入 chat 楼层。
- 实现 readDiff、rollback、listRuns、resume-run、profile routing、autoCommit/streaming 的明确策略。

## 每次 Agent 相关变更必须更新

新增或修改 Agent 相关实现时，请在本文件补充：

- 当前 phase 与状态变化。
- 涉及的 Rust/前端文件路径。
- 新增或变更的 Host ABI。
- 是否影响 Legacy Generate。
- 是否影响 windowed payload 保存契约。
- 新增测试与验证命令。
- 已知风险和后续待办。

## 守护契约

后续进度记录必须显式关注：

- Agent Mode off 时 Legacy `Generate()` 行为不变。
- LLM 调用不绕过 `ChatCompletionService`、LLM API log、proxy、secret、iOS policy。
- Agent 工具结果不写入 chat 楼层。
- Agent run/timeline event 不伪装成 SillyTavern `GENERATION_*` / `TOOL_CALLS_*` 事件。
- Commit/rollback 遵守 windowed payload 与保存串行化契约。
- MCP stdio command 不由 Agent/Preset/角色卡/世界书直接写入。
