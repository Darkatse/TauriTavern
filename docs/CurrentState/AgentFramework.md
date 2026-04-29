# Agent Framework 当前进度

本文档记录 Agent 框架的实时开发状态。它是“现在代码实际能做什么”的入口，不替代 `docs/AgentArchitecture.md`、`docs/AgentContract.md`、`docs/AgentImplementPlan.md` 与 `docs/Agent/` 下的设计文档。

## 当前状态

截至 2026-04-29，Phase 2C 已落地为当前基线：

- Rust 后端已有 Agent domain model、runtime、workspace、journal、checkpoint、commit bridge。
- Phase 2C 新增上下文只读工具：`chat.search`、`chat.read_messages`、`worldinfo.read_activated`。
- Chat workspace 级 `persist/` 持久 root 已落地：run 初始化时投影到本次 run，`finalizeCommit()` 成功后才写回稳定 chat workspace。
- `window.__TAURITAVERN__.api.agent` 已挂载最小可用 Host ABI。
- `window.__TAURITAVERN__.api.mcp` 尚未实现。
- Legacy Generate 尚未切换到 Agent 路径；当前 Agent 可通过 `api.agent.startRunFromLegacyGenerate()` 使用 dryRun adapter 启动，或通过 `api.agent.startRunWithPromptSnapshot()` 传入手工 prompt snapshot 测试。

当前能力可以概括为：

- 前端提供 `api.agent.startRunFromLegacyGenerate()`、`startRunWithPromptSnapshot()`、`cancel()`、`readEvents()`、`readWorkspaceFile()`、`subscribe()`、`prepareCommit()`、`finalizeCommit()`、`commit()`。
- `startRunFromLegacyGenerate()` 使用 Legacy `Generate(..., dryRun = true)` 捕获当前 SillyTavern prompt 语义，构造 `promptSnapshot.chatCompletionPayload`，捕获本轮 dryRun 最终 `worldInfoActivation`，并禁用 Legacy ToolManager tools。
- `startRunWithPromptSnapshot()` 会在前端解析 `stableChatId`，后端只接受非空稳定聊天身份。
- 后端为每次执行生成独立 `runId`，并由 `kind + stableChatId` 派生稳定 `workspaceId`。
- 后端复用现有 `ChatCompletionService` 调用 LLM，不直接绕过 provider 解析、secret、proxy、日志、iOS policy、endpoint policy 或取消注册。
- Runtime 会初始化 workspace、保存 prompt snapshot、投影 chat 级 `persist/`、写入 append-only run event、执行 `model -> tool -> model -> finish` 循环。
- Agent 可以按需搜索/读取当前聊天历史，读取本轮激活世界书条目，写出 workspace 草稿与最终 artifact，创建 checkpoint，并进入 `awaiting_commit`。
- commit 目前由前端桥接：`prepare_agent_run_commit` 生成 draft，前端调用 SillyTavern `saveReply()` 写入当前聊天，再调用 `finalize_agent_run_commit` 完成 run；只有 finalize 成功后，本 run 对 `persist/` 的变更才 promote 回 chat workspace。
- 当前明确拒绝 stream、autoCommit、external tools/tool turns；diff UI、rollback、resume、profile routing、Skill/MCP 仍是后续阶段。

## 当前工具集

内部 canonical name 使用 dotted form；发给 OpenAI-compatible function calling 时使用 provider-safe alias。

| Canonical name | Model-facing alias | 状态 | 说明 |
| --- | --- | --- | --- |
| `chat.search` | `chat_search` | 已落地 | 搜索当前 run 绑定的角色/群聊聊天文件。只有 `query` 必填；可选 `limit`、`role`、`start_message`、`end_message`、`scan_limit`。返回 message index、snippet、score 与 `chat:current#<index>` ref。 |
| `chat.read_messages` | `chat_read_messages` | 已落地 | 按 0-based message index 读取当前聊天消息。`messages` 数组每项必填 `index`，可选 `start_char`、`max_chars` 读取长消息片段。 |
| `worldinfo.read_activated` | `worldinfo_read_activated` | 已落地 | 读取本次 run 的 `promptSnapshot.worldInfoActivation`。模型可读文本只输出条目名、世界书名、条目内容；结构化结果保留 audit/ref 字段。 |
| `workspace.list_files` | `workspace_list_files` | 已落地 | 列出模型可见 workspace 文件；可见前缀由 manifest roots 决定，当前为 `output/`、`scratch/`、`plan/`、`summaries/`、`persist/`。 |
| `workspace.read_file` | `workspace_read_file` | 已落地 | 读取 UTF-8 文本文件并返回行号；完整读取会记录 read-state。 |
| `workspace.write_file` | `workspace_write_file` | 已落地 | 写 UTF-8 文本到 run workspace；当前可写前缀由 manifest roots 决定，包含 `persist/` 投影。 |
| `workspace.apply_patch` | `workspace_apply_patch` | 已落地 | Claude Code 风格单文件精确替换；要求已完整读取或由本 run 创建/修改。 |
| `workspace.finish` | `workspace_finish` | 已落地 | 结束工具循环；默认 final artifact 是 `output/main.md`。 |

当前没有 `skill.list`、`skill.read`、MCP 工具、shell 工具或外部 extension tools。

工具预算与错误边界：

- `chat.search` 默认返回 20 条，最大 50 条，`scan_limit` 最大 100000。0 命中是成功结果，不是错误。
- `chat.read_messages` 单次最多 20 条；完整读取单条上限 8000 字符；带范围读取单条上限 8000 字符；总返回上限 20000 字符。
- `worldinfo.read_activated` 默认返回 20000 字符内容，最大 50000 字符；没有本轮激活快照会返回 recoverable tool error。
- 模型参数、workspace path 字符串、可见/可写策略、文件不存在、message index 不存在、读取范围非法、结果过大、`workspace.apply_patch` 未读/非完整读取/sha 变化/匹配 0 次或多次等可修复问题会作为 recoverable tool error 返回，并回填下一轮模型。
- repository 内部 IO、journal、checkpoint、chat JSONL 损坏、序列化、取消和模型响应结构错误仍是 fatal runtime error。

## 重要边界

- Agent Mode off 时 Legacy `Generate()` 行为不变。
- `Generate(..., dryRun = true)` 本身不返回 prompt payload；它通过 `GENERATE_AFTER_DATA` 事件暴露 `generate_data`，然后 resolve `undefined`。Agent adapter 负责监听该事件并构造 `promptSnapshot.chatCompletionPayload`。
- `worldInfoActivation` 来自本次 Agent dryRun 的最终 `WORLDINFO_SCAN_DONE` 事件，不读取全局 last activation 当作真相源。
- 不存在公共 `api.agent.startRun()` alias。当前公共启动入口只有 `startRunFromLegacyGenerate()` 与 `startRunWithPromptSnapshot()`。
- Agent timeline/event 不伪装为 SillyTavern `GENERATION_*` 或 `TOOL_CALLS_*` 事件。
- 当前没有 Agent mode toggle、最小 timeline UI 或主发送按钮接入。
- 当前没有 `GenerationIntent + ContextFrame` 原生组装；Phase 2C 仍使用 `promptSnapshot.chatCompletionPayload` 作为过渡输入。
- 当前没有 MCP Host ABI；MCP 仍是后续目标。
- 当前 Agent ABI 的 `AgentChatRef` 在边界只接受/输出 `characterId`、`fileName`、`chatId`；内部 snake_case 字段名进入 ABI 会 fail-fast。

## 文档入口

- 高层架构：`docs/AgentArchitecture.md`
- 硬契约：`docs/AgentContract.md`
- 实施计划：`docs/AgentImplementPlan.md`
- 细节文档目录：`docs/Agent/README.md`
- Agent API 草案：`docs/API/Agent.md`
- MCP API 草案：`docs/API/MCP.md`

## 进度台账

| 日期 | 阶段 | 状态 | 变更/PR | 备注 |
| --- | --- | --- | --- | --- |
| 2026-04-26 | 基线 | 已并入当前架构 | 本地工作区 | 原 Phase 0/1/2A 的文档、domain/runtime/storage、workspace、journal、checkpoint、commit bridge、Host ABI、最小 tool loop 与 dryRun adapter 已吸收为当前基线。 |
| 2026-04-26 | Workspace 读改 | 已落地 | 本地工作区 | `workspace.list_files/read_file/write_file/apply_patch/finish`、read-state guard、patch checkpoint。 |
| 2026-04-29 | Workspace Persist | 已落地 | 本地工作区 | manifest roots、chat 级 `persist/` projection、commit-time promote、并发冲突检测。 |
| 2026-04-29 | 上下文只读工具 | 已落地 | 本地工作区 | `chat.search`、`chat.read_messages`、`worldinfo.read_activated`、run-scoped world info activation capture。 |

## 实施检查表

| 项目 | 状态 | 代码入口 | 测试/验证 | 备注 |
| --- | --- | --- | --- | --- |
| Agent domain models | 已落地 | `src-tauri/src/domain/models/agent/mod.rs` | `cargo test --manifest-path src-tauri/Cargo.toml agent --lib` | `AgentRun` / `AgentRunEvent` / `WorkspacePath` / manifest roots / artifact / checkpoint；ABI 使用 camelCase。 |
| Repository traits | 已落地 | `src-tauri/src/domain/repositories/agent_run_repository.rs`、`workspace_repository.rs`、`checkpoint_repository.rs`、`chat_repository.rs`、`group_chat_repository.rs` | `cargo check --manifest-path src-tauri/Cargo.toml` | application layer 依赖 trait；chat/group repository 提供后端消息搜索与按 index 读取。 |
| FileAgentRepository | 已落地 | `src-tauri/src/infrastructure/repositories/file_agent_repository.rs` | `repository_round_trips_run_workspace_event_and_checkpoint`、`persistent_workspace_projects_run_changes_only_after_commit` | 根目录为 `_tauritavern/agent-workspaces`；保存 run index、manifest、JSONL event、workspace file、checkpoint、chat 级 `persist/`。 |
| FileChatRepository message read/search | 已落地 | `src-tauri/src/infrastructure/repositories/file_chat_repository/message_search.rs`、`message_read.rs` | `read_character_chat_messages_returns_selected_messages_and_total_count`、`read_group_chat_messages_uses_message_indexes_without_header` | JSONL 第一行 header 不计入 message index；消息从 0 开始编号。 |
| AgentRuntimeService | 已落地 | `src-tauri/src/application/services/agent_runtime_service.rs`、`src-tauri/src/application/services/agent_runtime_service/` | `cargo test --manifest-path src-tauri/Cargo.toml agent --lib` | 已拆分 lifecycle/executor/loop/model_turn/tool_execution/journal 等子模块。 |
| LLM gateway wrapper | 部分落地 | `AgentRuntimeService` 复用 `ChatCompletionService::generate_with_cancel` | `cargo check --manifest-path src-tauri/Cargo.toml` | 还没有 provider-agnostic `ModelRequest`/`LlmGatewayService` 抽象。 |
| Tauri commands | 已落地 | `src-tauri/src/presentation/commands/agent_commands.rs`、`registry.rs` | `cargo check --manifest-path src-tauri/Cargo.toml` | `start/cancel/readEvents/readWorkspaceFile/prepareCommit/finalizeCommit`。 |
| `api.agent` Host ABI | 已落地 | `src/tauri/main/api/agent.js`、`src/tauri/main/api/agent-prompt-snapshot.js`、`src/tauri/main/bootstrap.js`、`src/types.d.ts` | `pnpm run check:types`、`pnpm run check:frontend` | `startRunFromLegacyGenerate()` 为 dryRun adapter；`subscribe()` 当前为 polling；future API 显式 throw。 |
| Commit bridge | 已落地 | `src/tauri/main/api/agent.js`、`agent_runtime_service/commit.rs` | `finalize_commit_promotes_persistent_workspace_projection` | 前端 `saveReply()` 写 chat，合并 `tauritavern.agent` metadata，再 finalize；finalize 后 promote `persist/`。 |
| Stable chat identity | 已落地 | `active-chat-ref.js`、`chat.js`、`agent.js`、`AgentRuntimeService` | `workspace_id_uses_stable_chat_id_not_character_chat_file_name` | `workspaceId = chat_ + sha256(kind + stableChatId)[0..16]`。 |
| ToolRegistry/ToolDispatch | 已落地 | `src-tauri/src/application/services/agent_tools.rs`、`src-tauri/src/application/services/agent_tools/`、`agent_runtime_service/tool_execution.rs` | `dispatcher_searches_and_reads_current_chat_messages`、`dispatcher_reads_worldinfo_activation_from_run_snapshot` | 当前开放 chat/worldinfo/workspace 八个内建工具；workspace 可见/可写根由 manifest roots 驱动。 |
| 最小 timeline UI | 未开始 | - | - | 不伪装成 SillyTavern `GENERATION_*` 事件。 |
| `api.mcp` Host ABI | 未开始 | - | - | MCP 独立于 Agent Mode。 |

## 当前后端运行流

```text
api.agent.startRunFromLegacyGenerate(input?)
  ↓
Legacy dryRun 捕获 GENERATE_AFTER_DATA 与最终 WORLDINFO_SCAN_DONE
  ↓
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
dispatch chat.search / chat.read_messages / worldinfo.read_activated / workspace.*
  ↓
tool result 回填下一轮 request，直到 workspace.finish
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
persistent_projection_initialized
context_assembled
model_request_created
model_completed
tool_call_requested
tool_call_started
tool_result_stored
tool_call_completed / tool_call_failed
workspace_file_written
workspace_patch_applied
checkpoint_created
agent_loop_finished
artifact_assembled
commit_started
persistent_changes_prepared / persistent_changes_prepare_failed
commit_draft_created
persistent_changes_committed / persistent_changes_commit_failed
run_committed
run_completed
run_cancel_requested
run_cancelled
run_failed
```

工具循环最多 80 轮；超过后会以 `agent.max_tool_rounds_exceeded` 失败。当前仍要求模型必须调用工具；如果模型直接输出文本且不调用工具，会以代码中的 `model.tool_call_required_phase2b` 失败。该错误码是历史命名遗留，不代表当前能力边界。

## 当前文件布局

Agent workspace 由 `FileAgentRepository` 管理，当前物理布局：

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
            persist_snapshot.json
          tool-args/
            <tool-call-id>.json
          tool-results/
            <tool-call-id>.json
          output/
            main.md
          scratch/
          plan/
          summaries/
          persist/
            <run projection of chat-level persist files>
          checkpoints/
            <checkpoint-id>/
              checkpoint.json
              <snapshotted workspace files...>
```

Workspace path 必须是相对路径。`workspace.list_files` 的 `path` 省略、空字符串、空白字符串、`.`、`./` 表示 workspace root；其他工具仍要求非空文件路径。绝对路径、Windows drive prefix、NUL、`..` 会被 workspace path parser 拒绝，并在工具参数层作为可恢复 tool error 回传模型。

当前 manifest roots 使模型可见 / 可写：

```text
output/
scratch/
plan/
summaries/
persist/
```

`persist/` 是 chat workspace 级持久 root 的 run projection。run 中修改 `persist/` 只影响本 run；`prepareCommit()` 会预检 persistent changes 与并发冲突，`finalizeCommit()` 成功后才 promote 回 `chats/<workspace-id>/persist/`。Failed、Cancelled 或未 finalize 的 run 不会污染下一轮。

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

### Phase 2C 上下文工具 smoke

用于验证 `chat_search`、`chat_read_messages`、`worldinfo_read_activated`、workspace 写入与 finish 是否真实可用。当前聊天需要有可搜索内容；若要验证世界书非空结果，当前输入需要触发至少一条世界书。

```js
(async () => {
  const tt = window.__TAURITAVERN__;
  await tt.ready;

  const agent = tt.api.agent;
  const searchQuery = 'Agent';

  const quietPrompt = [
    'This is a TauriTavern Agent Phase 2C context-tools smoke test.',
    '',
    'You must complete the task using tools only.',
    'Do not answer directly in chat text.',
    'Call exactly one tool per assistant turn. Wait for the tool result before the next step.',
    '',
    'Required workflow:',
    `1. Call chat_search with query "${searchQuery}".`,
    '2. Call chat_read_messages using one message index returned by chat_search. If chat_search returns no hits, read message index 0 with max_chars 1200.',
    '3. Call worldinfo_read_activated with no arguments.',
    '4. Call workspace_write_file to write a concise Chinese report to output/main.md. The report must mention whether chat search, message read, and world info read returned usable data.',
    '5. Call workspace_finish with final_path set to output/main.md.',
  ].join('\n');

  const run = await agent.startRunFromLegacyGenerate({
    chatRef: tt.api.chat.current.ref(),
    generationType: 'quiet',
    generateOptions: { quiet_prompt: quietPrompt },
    options: { stream: false, autoCommit: false },
  });

  console.log('[agent run]', run);
  window.__agentRun = run;
  window.__agentStop?.();
  window.__agentStop = agent.subscribe(run.runId, async e => {
    console.log('[agent]', e.seq, e.type, e.payload);
    if (e.type === 'agent_loop_finished') {
      const finalFile = await agent.readWorkspaceFile({
        runId: run.runId,
        path: 'output/main.md',
      });
      console.log('[agent final]', finalFile.text);
    }
  });
})().catch(console.error);
```

### Workspace 读改 smoke

用于验证多轮工具回填、workspace 写入、list/read/apply_patch、checkpoint 与最终 artifact：

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
      'This is a TauriTavern Agent workspace read/write/patch smoke test.',
      '',
      'You must complete the task using workspace tools only.',
      'Do not answer directly in chat text.',
      'Call exactly one tool per assistant turn. Wait for the tool result before the next step.',
      '',
      'Required workflow:',
      '1. Call workspace_write_file to write an outline to plan/outline.md.',
      '2. Call workspace_write_file to write a complete draft to scratch/draft.md.',
      '3. Call workspace_list_files with an omitted or empty path to inspect the visible workspace.',
      '4. Call workspace_read_file to fully read scratch/draft.md.',
      '5. Call workspace_apply_patch to revise one exact passage in scratch/draft.md.',
      '6. Call workspace_read_file to fully read the revised scratch/draft.md.',
      '7. Call workspace_write_file to write concise revision notes to summaries/revision_notes.md.',
      '8. Call workspace_write_file to write the polished final answer to output/main.md.',
      '9. Call workspace_finish with final_path set to output/main.md.',
      '',
      'The final answer topic is:',
      'Explain why an Agent loop is better than one-shot generation for a Tauri-native SillyTavern fork.',
      '',
      'The final output must be polished Chinese prose, about 300-500 Chinese characters.',
    ].join('\n'),
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

## 最近验证命令

Phase 2C 实现落地时最近一次 Rust 侧验证：

- `cargo fmt --manifest-path src-tauri/Cargo.toml`：passed。
- `cargo check --manifest-path src-tauri/Cargo.toml`：passed。
- `cargo test --manifest-path src-tauri/Cargo.toml`：467 passed。
- `cargo test --manifest-path src-tauri/Cargo.toml dispatcher_reads_worldinfo_activation_from_run_snapshot`：passed。

前端检查：

- `pnpm run check:types`：passed。
- `pnpm run check:frontend`：passed。
- `pnpm run check:contracts`：当前因缺少既有 `docs/AccessibilitySmokeChecklist.md` 失败，与 Phase 2C 代码无关；修复该文档缺口后应重新运行。

## 已知待办

- 将 Agent run 接入可控 UI，而不是只靠控制台调用。
- 设计 Agent Mode toggle 与 Legacy Generate 的清晰分流，不改变 Agent Mode off 语义。
- 建立最小 timeline/event viewer。
- 将 `promptSnapshot` 过渡输入逐步替换为 `GenerationIntent + ContextFrame`。
- 拆出 provider-agnostic LLM gateway/model request，继续保留 `ChatCompletionService` 的既有策略链。
- 实现 `skill.list` / `skill.read`，并把 Preset、角色卡作者、扩展作者提供的 Skill-like 资源纳入统一接口。
- 实现 readDiff、rollback、listRuns、resume-run、profile routing、autoCommit/streaming 的明确策略。

## 每次 Agent 相关变更必须更新

新增或修改 Agent 相关实现时，请在本文件补充：

- 当前状态变化。
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
