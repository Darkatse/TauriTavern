# TauriTavern Agent Testing Strategy

本文档定义 Agent 系统的测试策略。Agent 涉及生成、文件、工具、外部协议、保存与兼容事件，测试必须持续作为开发入口的一部分。

## 1. 测试目标

测试要守住：

- Legacy Generate 兼容。
- Clean Architecture 边界。
- Workspace path 安全。
- Journal 完整性。
- Windowed payload 保存契约。
- LLM gateway 不绕过现有 policy/logging。
- Tool policy 与 approval。
- MCP 安全边界。
- 移动端内存与分页读取。

## 2. Domain Tests

覆盖：

```text
WorkspacePath normalization
WorkspacePath traversal rejection
Artifact manifest validation
Required artifact missing
AgentRunStatus transitions
AgentRunEvent serialization
PlanPolicy strict/free/hybrid
ToolPolicy allow/deny/approval
Profile resolution precedence
Checkpoint metadata
```

Domain tests 不应需要 Tauri、文件系统或 HTTP。

## 3. Application Tests

使用 mock repositories/gateway/tools。

覆盖：

```text
agent loop success
agent loop model failure
cancel before model call
cancel during model call
workspace write creates checkpoint
artifact assembly success/failure
commit service called through expected boundary
tool loop success
tool recoverable error
tool policy denied
plan locked node violation
profile switch allowed/denied
agent.list policy filtering
agent.delegate creates task and child invocation
agent.delegate schedules return-mode child task on background scheduler
agent.await waits for background child task and renders result capsule
completed child results are injected into the next parent model turn once
workspace.finish cancels unfinished child tasks without blocking run completion
task.return records result and terminates child invocation
child invocation cannot commit, finish, or delegate
child workspace policy keeps paths unchanged and only scopes visible/writable roots
```

关键断言：

- 每个副作用都有 journal event。
- failure 后状态为 `Failed`。
- cancel 后状态为 `Cancelled`。
- required artifact 缺失不 commit。

## 4. Infrastructure Tests

覆盖：

```text
file event journal append/read pagination
checkpoint snapshot restore
workspace repository rejects symlink escape
workspace repository handles unicode relative paths
file sizes and retention
MCP config allowlist
SkillRepository preview/install/read/export/source refs
Skill archive roundtrip hash
Skill repository rejects symlink escape
```

文件测试应使用临时目录，并覆盖 macOS/Linux/Windows path 差异。

## 5. LLM Gateway Tests

覆盖：

```text
gateway calls ChatCompletionService, not HttpChatCompletionRepository
source denied by iOS policy
endpoint override denied
prompt cache hints preserved
LLM API log wrapper remains in path
LLM API log readable output separates visible reasoning from assistant text
stream chunk becomes model_delta event
cancel propagates
tool_call_id opaque round-trip
native metadata round-trip
canonical AgentModelRequest/AgentModelResponse encode-decode
recent workspace write/patch tool result hydration
tool args/results use short hashed local audit file names while preserving opaque provider tool_call_id
```

特别要覆盖 `docs/CurrentState/NativeApiFormats.md` 中的契约：

- tool_call_id 不透明。
- Claude / Gemini / OpenAI Responses / Gemini Interactions native metadata 保真。
- Custom Claude header 策略不被硬编码覆盖。

## 6. Frontend Contract Tests

覆盖：

```text
window.__TAURITAVERN__.api.agent exists after ready
window.__TAURITAVERN__.api.skill exists after ready
window.__TAURITAVERN__.api.mcp exists after ready when MCP Host ABI lands
subscribe returns idempotent unsubscribe
Agent API uses safeInvoke, not raw command dependency in public caller
types.d.ts includes agent and skill types; mcp types land with MCP Host ABI
```

Legacy 回归：

```text
Agent mode off: Generate signature unchanged
Agent mode off: GENERATION_STARTED order unchanged
Agent mode off: GENERATE_AFTER_DATA dryRun still emitted
Agent mode off: ToolManager legacy behavior unchanged
Agent event does not emit fake GENERATION_* events
```

当前 Agent Host ABI 与工具循环必须覆盖：

```text
api.agent exposes startRunFromLegacyGenerate and startRunWithPromptSnapshot
api.agent does not expose ambiguous startRun alias
Generate(..., dryRun = true) resolves undefined and emits GENERATE_AFTER_DATA
startRunFromLegacyGenerate captures dryRun payload through event listener
agentMode disables Legacy ToolManager tools in prompt snapshot
Agent initialChatHistoryMessages positive window keeps latest-first recent turns before PromptManager assembly
Agent PromptManager assembly materializes a working copy and does not mutate FrozenRunInputSnapshot.promptInputs.messages
external tools/tool_choice/tool turns are rejected
stream true is rejected
foreground finish before workspace.commit returns recoverable tool error
workspace.commit append without prior commit creates the run message
subscribe polling can read events in seq order
readWorkspaceFile returns UTF-8 text, chars, words, sha256
readModelTurn returns assistant text, visible reasoning, tool calls, provider summary
workspace_list_files accepts omitted/empty/dot path as workspace root
workspace_search_files searches only visible roots and returns snippets
workspace_read_file full read records read-state
workspace_read_file character range does not unlock patch state unless it covers the full file
workspace_write_file append creates missing files and appends existing files without a rewrite read
workspace_write_file append does not auto-insert newlines and does not unlock rewrite or patch state for unread existing content
workspace_apply_patch requires full read-state and checkpoints on success
chat deletion removes the corresponding Agent chat workspace and run index
chat deletion fails clearly while the corresponding Agent workspace has an active run
skill_search respects visible/deny policy and read budget
skill_read supports line and character ranges
recoverable tool errors are returned to the model instead of failing the run
listRuns returns paginated Agent run history summaries
future APIs approveToolCall/readDiff/rollback throw explicitly
run timeline/detail view switching has a single explicit state source and does not derive detailsOpen from scrollLeft
run timeline main panel does not use horizontal scroll-snap or smooth scroll as a view state machine
run timeline closes detail by resetting detail state and does not measure or auto-stick hidden timeline scrollers
```

Provider normalizer tests 必须覆盖可见 reasoning 提取：Claude `thinking`、Gemini `thought` 文本、OpenAI Responses reasoning summary 进入 `reasoning_content`；signature / encrypted continuation 仍作为 native/provider state 保留，不能作为可展示文本。

## 7. Windowed Payload Integration Tests

覆盖：

```text
Agent reads history through windowed/search APIs
Agent does not expand UI chat window
Agent commit uses chat save contract
Agent commit does not trigger cursor mismatch under serialized saves
cursor mismatch fails clearly
force does not bypass cursor signature
rollback committed message uses save contract
```

## 8. Security Tests

覆盖：

```text
../ path rejected
absolute path rejected
Windows drive path rejected
symlink escape rejected
hidden resource not in context
denied tool not visible
denied tool call fails
MCP arbitrary stdio command rejected
Agent cannot edit MCP config
extension tool without authorization hidden
provider source denied by policy
```

## 9. Performance Tests

覆盖：

```text
large chat history remains virtual
journal pagination does not load full file
workspace tree lazy read
checkpoint retention cap
tool result budget truncation/summary
mobile default budgets
```

指标建议：

- Agent run workspace 初始化耗时。
- Journal append/read latency。
- Large history Agent start memory growth。
- Timeline first render event count。

## 10. Golden Fixtures

建议建立 fixtures：

```text
fixtures/agent/
  prompt_snapshot_openai.json
  prompt_snapshot_claude.json
  run_events_one_step.jsonl
  manifest_main_only.json
  manifest_multi_artifact.json
  tool_result_chat_search.json
  checkpoint_snapshot/
```

Golden fixtures 应尽量脱敏，不包含真实 API key 或私人聊天。

## 11. Merge Gates

当前落地门禁：

- 后端 `cargo check --manifest-path src-tauri/Cargo.toml` 通过。
- 后端 `cargo test --manifest-path src-tauri/Cargo.toml agent_runtime_service` 通过。
- 后端 `cargo test --manifest-path src-tauri/Cargo.toml file_agent_repository` 通过。
- 后端 `cargo test --manifest-path src-tauri/Cargo.toml file_agent_profile_repository` 通过。
- 涉及前端 ABI 时，前端 `pnpm run check:types`、`pnpm run check:contracts`、`pnpm run check:frontend` 通过。
- 控制台 smoke 能通过 `startRunFromLegacyGenerate()` 启动 run。
- 控制台 Agent smoke 能依次调用 `chat_search`、`chat_read_messages`、`worldinfo_read_activated`，写入 `output/main.md` 并进入 `awaiting_commit`。
- `cargo test agent_model_gateway`、`cargo test openai_responses_payload`、`cargo test claude_native_content_blocks_are_replayed`、`cargo test normalize_` 通过。
- `provider_state` / gateway 相关测试覆盖 OpenAI Responses `previousResponseId` / `messageCursor`、same-provider native metadata loss fail-fast、cross-provider private metadata 不迁移、LLM API log 剥离 `_tauritavern_provider_state`。
- 控制台 workspace 读改 smoke 能依次写入 `plan/outline.md`、`scratch/draft.md`，调用 `workspace_list_files`，完整读取 draft，使用 `workspace_apply_patch` 修改 draft，写入 `summaries/revision_notes.md`、`output/main.md` 并进入 `awaiting_commit`。
- `commit()` 能把 `output/main.md` 写入当前 active chat；`workspace.finish` 成功后才把 durable `persistStateId` 写回该消息，并追加 `run_completed`。
- Agent Mode off 的 Legacy Generate 行为不变。

后续工具/运行时变更不合并，除非：

- tool loop 测试通过。
- tool result 不写 chat message。
- recoverable tool error 回填模型测试通过。
- workspace path security 测试通过。

Profile / Plan 相关变更不合并，除非：

- profile resolution 测试通过。
- `input/resolved_profile.json` 快照写入测试通过。
- tool/skill/workspace/output policy 的 runtime 行为测试通过。
- `agentSystemPrompt` 前端 materialize、PromptManager index/role 保留、runtime marker 泄漏 fail-fast 测试通过。
- strict/free/hybrid plan 与 profile switch 相关测试在对应功能实现时补齐。

MCP 相关变更不合并，除非：

- MCP stdio command allowlist 测试通过。
- dangerous tool approval 测试通过。
- Agent 不能编辑 MCP config。
