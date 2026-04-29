# TauriTavern Agent Testing Strategy

本文档定义 Agent 系统的测试策略。Agent 涉及生成、文件、工具、外部协议、保存与兼容事件，测试必须从第一阶段就建立。

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
SkillRepository list/read
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
stream chunk becomes model_delta event
cancel propagates
tool_call_id opaque round-trip
native metadata round-trip
```

特别要覆盖 `docs/CurrentState/NativeApiFormats.md` 中的契约：

- tool_call_id 不透明。
- Gemini native metadata 保真。
- Custom Claude header 策略不被硬编码覆盖。

## 6. Frontend Contract Tests

覆盖：

```text
window.__TAURITAVERN__.api.agent exists after ready
window.__TAURITAVERN__.api.mcp exists after ready (Phase 5)
subscribe returns idempotent unsubscribe
Agent API uses safeInvoke, not raw command dependency in public caller
types.d.ts includes agent types; mcp types are Phase 5
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
external tools/tool_choice/tool turns are rejected
stream true and autoCommit true are rejected
subscribe polling can read events in seq order
readWorkspaceFile returns UTF-8 text, bytes, sha256
workspace_list_files accepts omitted/empty/dot path as workspace root
workspace_read_file full read records read-state
workspace_apply_patch requires full read-state and checkpoints on success
recoverable tool errors are returned to the model instead of failing the run
future APIs approveToolCall/listRuns/readDiff/rollback throw explicitly
```

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

## 11. Phase Gates

当前落地门禁：

- 后端 `cargo test --manifest-path src-tauri/Cargo.toml agent --lib` 通过。
- 后端 `cargo check --manifest-path src-tauri/Cargo.toml` 通过。
- 前端 `pnpm run check:types`、`pnpm run check:contracts`、`pnpm run check:frontend` 通过。
- 控制台 smoke 能通过 `startRunFromLegacyGenerate()` 启动 run。
- 控制台 workspace 读改 smoke 能依次写入 `plan/outline.md`、`scratch/draft.md`，调用 `workspace_list_files`，完整读取 draft，使用 `workspace_apply_patch` 修改 draft，写入 `summaries/revision_notes.md`、`output/main.md` 并进入 `awaiting_commit`。
- `commit()` 能把 `output/main.md` 写入当前 active chat，并追加 `run_committed` / `run_completed`。
- Agent Mode off 的 Legacy Generate 行为不变。

后续工具/运行时变更不合并，除非：

- tool loop 测试通过。
- tool result 不写 chat message。
- recoverable tool error 回填模型测试通过。
- workspace path security 测试通过。

Phase 3 不合并，除非：

- profile resolution 测试通过。
- strict/free/hybrid plan 测试通过。
- profile switch journal 测试通过。

Phase 5 不合并，除非：

- MCP stdio command allowlist 测试通过。
- dangerous tool approval 测试通过。
- Agent 不能编辑 MCP config。
