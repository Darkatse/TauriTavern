# `window.__TAURITAVERN__.api.agent` — Agent API

本文档是 Agent Host ABI 草案。它描述前端/扩展可见的稳定入口，不等同于 Rust 内部 service/repository。

状态：当前已实现 canonical model IR、provider native metadata 保真、provider_state continuation、上下文只读工具、workspace 读改工具循环与前端 dryRun adapter。本文以当前已落地 Host ABI 为准，并在后续章节保留 readDiff/rollback/approval/listRuns 等未来草案。

`provider_state` 是 Rust 后端内部 continuation contract，不是 Host ABI。前端/扩展不应读写 `_tauritavern_provider_state`；需要诊断时通过 run events、`modelResponsePath` 与 LLM API log 观察。

## 1. 入口

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);

const agent = window.__TAURITAVERN__.api.agent;
```

Agent API 必须挂在 `window.__TAURITAVERN__.api.agent`。不要新增散落全局。

## 2. 当前方法概览

```ts
type TauriTavernAgentApi = {
  startRunFromLegacyGenerate(input?: AgentStartRunFromLegacyGenerateInput): Promise<AgentRunHandle>;
  startRunWithPromptSnapshot(input: AgentStartRunWithPromptSnapshotInput): Promise<AgentRunHandle>;
  subscribe(runId: string, handler: (event: AgentRunEvent) => void, options?: AgentSubscribeOptions): TauriTavernHostUnsubscribe;
  cancel(runId: string): Promise<AgentRunHandle>;
  readEvents(input: AgentReadEventsInput): Promise<AgentReadEventsResult>;
  readWorkspaceFile(input: AgentReadWorkspaceFileInput): Promise<AgentWorkspaceFile>;
  commit(input: AgentCommitInput): Promise<AgentCommitResult>;
  prepareCommit(input: AgentPrepareCommitInput): Promise<AgentCommitDraft>;
  finalizeCommit(input: AgentFinalizeCommitInput): Promise<AgentCommitResult>;

  approveToolCall(): never;
  listRuns(): never;
  readDiff(): never;
  rollback(): never;
};
```

`subscribe()` 返回的 unsubscribe 必须幂等。

当前没有公共 `startRun()` alias。启动职责必须一眼可见：

- `startRunFromLegacyGenerate()`：从当前 Legacy Generate dryRun 兼容桥启动。
- `startRunWithPromptSnapshot()`：调用方已经持有 prompt snapshot 时启动。

`approveToolCall()`、`listRuns()`、`readDiff()`、`rollback()` 已预留名称，但当前实现会显式 throw，避免静默降级。

## 3. startRunFromLegacyGenerate

```ts
type AgentStartRunFromLegacyGenerateInput = {
  chatRef?: AgentChatRef;
  stableChatId?: string;
  generationType?: 'normal' | 'regenerate' | 'swipe' | 'continue' | 'quiet' | 'impersonate';
  generateOptions?: unknown;
  profileId?: string;
  generationIntent?: AgentGenerationIntent;
  options?: {
    autoCommit?: false;
    stream?: false;
  };
};
```

`startRunFromLegacyGenerate()` 是当前前端兼容桥：它使用 Legacy `Generate(..., dryRun = true)` 捕获当前 SillyTavern prompt 语义，构造 `promptSnapshot.chatCompletionPayload`，捕获本轮 dryRun 最终 `worldInfoActivation`，再调用 `startRunWithPromptSnapshot()`。

要求：

- 只用于当前 active chat。
- 当前只支持 `main_api = openai` 的 chat-completion 路径。
- 必须禁用 Legacy ToolManager tools；Agent tools 只能由 Rust runtime 注册。
- `worldInfoActivation` 必须来自本次 dryRun 的最终 `WORLDINFO_SCAN_DONE`，不能读取全局 last activation 当作 run 真相。
- `stream` 与 `autoCommit` 必须为 `false` 或省略。
- dryRun 没有产出 messages、已有 tool turns、已有 external tools 都必须 reject，不回退 Legacy Generate。

注意：`Generate(..., dryRun = true)` 不返回 payload。它只 emit `GENERATE_AFTER_DATA`，然后 resolve `undefined`。调用方不应写 `const payload = await Generate(..., true)`；捕获逻辑由 `startRunFromLegacyGenerate()` 内部 adapter 负责。

## 4. startRunWithPromptSnapshot

```ts
type AgentStartRunWithPromptSnapshotInput = {
  chatRef: AgentChatRef;
  stableChatId?: string;
  generationType?: 'normal' | 'regenerate' | 'swipe' | 'continue' | 'quiet' | 'impersonate';
  profileId?: string;
  promptSnapshot: AgentPromptSnapshot;
  generationIntent?: AgentGenerationIntent;
  workspaceMode?: 'new-run' | 'resume-run';
  resumeRunId?: string;
  options?: {
    autoCommit?: boolean;
    stream?: boolean;
  };
};

type AgentPromptSnapshot = {
  chatCompletionPayload: unknown;
  worldInfoActivation?: {
    timestampMs: number;
    trigger: string;
    entries: Array<{
      world: string;
      uid: string | number;
      displayName: string;
      constant: boolean;
      content: string;
      position?: 'before' | 'after' | 'an_top' | 'an_bottom' | 'depth' | 'em_top' | 'em_bottom' | 'outlet';
    }>;
  };
};

type AgentRunHandle = {
  runId: string;
  status: AgentRunStatus;
  workspaceId: string;
  stableChatId: string;
};
```

身份语义：

- `stableChatId` 是聊天的长期稳定身份。
- `workspaceId` 必须由 `kind + stableChatId` 派生，不得由可变的 `chatRef` 文件名直接决定。
- `runId` 是一次 Agent 执行身份，每次 normal/regenerate/swipe/continue 都必须生成新的 `runId`。
- 同一稳定聊天的多次 run 应共享同一个 chat workspace，但各自拥有独立 run workspace。

Public Host ABI 可以允许调用方省略 `stableChatId`，但 `api.agent.startRunWithPromptSnapshot()` 必须在调用 Rust command 前通过 `api.chat.open(chatRef).stableId()` 解析并校验。Rust command 不应自行读取 SillyTavern metadata。

当前要求提供 `promptSnapshot.chatCompletionPayload`。`promptSnapshot.worldInfoActivation` 是可选字段，由 `worldinfo_read_activated` 读取；长期目标是 `generationIntent + ContextFrame`，但当前 Rust runtime 不会只凭 `generationIntent` 组装上下文。

要求：

- `stableChatId` 进入 backend DTO 前必须非空；无法解析时 fail-fast。
- `promptSnapshot.chatCompletionPayload` 必须包含 chat-completion payload object。
- 如果调用方希望 `worldinfo_read_activated` 返回非错误结果，必须在 prompt snapshot 中提供本次 run 的 `worldInfoActivation`。
- 当前拒绝已有 `tools`、`tool_choice`、`role: "tool"` 或已有 `tool_calls` 的外部 tool turns。
- 当前拒绝 `stream: true` 与 `autoCommit: true`。
- `workspaceMode` / `resumeRunId` 当前只是后续阶段字段，不应作为当前行为依赖。
- 参数无效必须 reject，不静默回退 Legacy Generate。

## 5. subscribe

```ts
type AgentSubscribeOptions = {
  afterSeq?: number;
  limit?: number;
  intervalMs?: number;
  onError?: (error: unknown) => void;
};
```

语义：

- 当前 `subscribe()` 是前端 polling wrapper，底层调用 `readEvents()`。
- 默认从 `afterSeq = 0` 开始读取；调用方可以传入 `afterSeq`。
- 返回 unsubscribe 函数，必须幂等。
- 底层 polling 细节和 Rust command 名不是 Public Contract。

## 6. cancel

```ts
await agent.cancel(runId);
```

语义：

- 写 `run_cancel_requested`。
- 尽力取消当前模型请求或工具调用。
- Cancel 不是 failure。
- Cancel 后不能自动 commit。
- 返回最新 `AgentRunHandle`。

## 7. approveToolCall

当前未实现审批流程；`approveToolCall()` 会显式 throw。

```ts
type AgentApproveToolCallInput = {
  runId: string;
  callId: string;
  approved: boolean;
  reason?: string;
};
```

语义：

- 审批结果写 journal。
- 拒绝工具不等同 run failure；具体后续由 plan/profile policy 决定。

## 8. readEvents

```ts
type AgentReadEventsInput = {
  runId: string;
  afterSeq?: number;
  beforeSeq?: number;
  limit?: number;
};

type AgentReadEventsResult = {
  events: AgentRunEvent[];
};
```

要求：

- `limit` 必须有上限。
- 移动端 UI 不应一次读取完整巨大 journal。
- 当前暂不返回 `hasMoreBefore/hasMoreAfter`。

## 9. readWorkspaceFile

```ts
type AgentReadWorkspaceFileInput = {
  runId: string;
  path: string;
  checkpointId?: string;
};

type AgentWorkspaceFile = {
  path: string;
  text: string;
  bytes: number;
  sha256: string;
};
```

路径必须是 workspace relative path。非法路径直接 reject。
当前 Host ABI 只读当前 run workspace 的 UTF-8 文本文件，不支持 `checkpointId` 参数。模型侧读取应使用 `workspace_read_file` 工具，前端/扩展侧读取应使用本方法。

## 10. readDiff

当前未实现 diff；`readDiff()` 会显式 throw。

```ts
type AgentReadDiffInput = {
  runId: string;
  fromCheckpointId?: string;
  toCheckpointId?: string;
  paths?: string[];
};

type AgentDiff = {
  runId: string;
  fromCheckpointId?: string;
  toCheckpointId?: string;
  files: Array<{
    path: string;
    status: 'added' | 'modified' | 'deleted' | 'unchanged';
    unifiedDiff?: string;
  }>;
};
```

第一期可以只支持文本 artifact 的 diff。

## 11. rollback

当前未实现 rollback；`rollback()` 会显式 throw。

```ts
type AgentRollbackInput = {
  runId: string;
  checkpointId: string;
  scope?: 'workspace' | 'committed-message';
};
```

语义：

- `workspace`：只恢复 run workspace，不修改 chat。
- `committed-message`：重组 artifact 并修改已提交聊天消息，必须走 chat 保存契约。

## 12. commit

```ts
type AgentCommitInput = {
  runId: string;
  messageId?: string | number;
};

type AgentCommitResult = {
  runId: string;
  status: AgentRunStatus;
};
```

Commit 必须：

- 读取 manifest。
- 校验 required artifact。
- 校验当前 active chat 与 run 的 `stableChatId` 一致。
- 通过既有 chat 保存契约写入。
- 写 agent metadata。
- 追加 `run_committed` event。

当前 `commit()` 是前端桥接 helper：先调用 `prepareCommit()`，校验当前 active chat 与 run 的 `chatRef/stableChatId` 一致，调用上游 `saveReply()` 写入聊天，再调用 `finalizeCommit()`。

`prepareCommit()` 返回 draft：

```ts
type AgentPrepareCommitInput = {
  runId: string;
};

type AgentCommitDraft = {
  runId: string;
  stableChatId: string;
  chatRef: AgentChatRef;
  generationType: string;
  checkpoint: unknown;
  message: {
    mes: string;
    extra?: unknown;
  };
};
```

`finalizeCommit()` 只允许在 backend run 状态为 `committing` 时调用：

```ts
type AgentFinalizeCommitInput = {
  runId: string;
  messageId?: string | number;
};
```

## 13. Event Envelope

```ts
type AgentRunEvent = {
  seq: number;
  id: string;
  runId: string;
  timestamp: string;
  level: 'debug' | 'info' | 'warn' | 'error';
  type: AgentRunEventType;
  payload: unknown;
};
```

事件类型见 `docs/Agent/RunEventJournal.md`。

Agent event 不等同 SillyTavern `eventSource` 事件，不得伪装成 `GENERATION_*` 或 `TOOL_CALLS_*`。

## 14. Errors

错误建议结构：

```ts
type AgentApiError = {
  code: string;
  message: string;
  runId?: string;
  eventSeq?: number;
  retryable?: boolean;
  details?: unknown;
};
```

常见 code：

```text
agent.invalid_intent
agent.invalid_profile
agent.policy_violation
agent.not_found
workspace.path_denied
workspace.required_artifact_missing
model.request_failed
tool.policy_denied
commit.cursor_integrity
commit.save_failed
```

## 15. Rust Command 草案

```text
start_agent_run(dto)
cancel_agent_run(dto)
read_agent_run_events(dto)
read_agent_workspace_file(dto)
prepare_agent_run_commit(dto)
finalize_agent_run_commit(dto)
```

Command 层必须是薄封装。Agent loop 不写在 command 内。

后续草案命令：

```text
approve_agent_tool_call(dto)
list_agent_runs(chat_ref)
read_agent_diff(dto)
rollback_agent_run(dto)
```

## 16. Compatibility

Agent Mode off：

- `Generate()` 行为不变。
- `ToolManager` 行为不变。
- `api.chat` 行为不变。

Agent Mode on：

- 短期可使用 dryRun 生成 prompt snapshot。
- dryRun 不是纯函数，调用方必须理解它仍会触发上游事件。
- dryRun 不返回 payload；Agent adapter 通过事件捕获 payload。
- Agent tool loop 不递归 `Generate()`。

## 17. 当前工具与手动验证

当前开放八个内建工具：

| Canonical name | Model-facing alias | 说明 |
| --- | --- | --- |
| `chat.search` | `chat_search` | 搜索当前 run 绑定的聊天。只有 `query` 必填；可选 `limit`、`role`、`start_message`、`end_message`、`scan_limit`。返回 message index、snippet 与 ref。 |
| `chat.read_messages` | `chat_read_messages` | 按 0-based message index 读取当前聊天消息；每项可选 `start_char`、`max_chars` 读取长消息片段。 |
| `worldinfo.read_activated` | `worldinfo_read_activated` | 读取本次 run 的最终激活世界书条目；模型可读文本只包含条目名、世界书名、条目内容。 |
| `workspace.list_files` | `workspace_list_files` | 列出模型可见 workspace 文件；`path` 省略、空字符串、`.`、`./` 表示 workspace root |
| `workspace.read_file` | `workspace_read_file` | 读取 UTF-8 文本文件并返回行号；完整读取记录 read-state |
| `workspace.write_file` | `workspace_write_file` | 写 UTF-8 文本到 manifest 可写 roots，当前为 `output/`、`scratch/`、`plan/`、`summaries/`、`persist/` |
| `workspace.apply_patch` | `workspace_apply_patch` | 单文件 `old_string` / `new_string` 精确替换，要求已完整读取或由本 run 创建/修改 |
| `workspace.finish` | `workspace_finish` | 结束工具循环，默认 final artifact 为 `output/main.md` |

当前不存在 `skill.list`、`skill.read`、MCP、shell 或 extension bridge 工具。

模型可修正的工具错误会作为 `is_error = true` tool result 回填下一轮。宿主级 IO、journal、checkpoint、序列化、取消和模型响应结构错误仍然让 run failed。

推荐最小启动：

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);

const agent = window.__TAURITAVERN__.api.agent;

const run = await agent.startRunFromLegacyGenerate({
  generationType: 'normal',
  options: { stream: false, autoCommit: false },
});

const stop = agent.subscribe(run.runId, event => console.log(event));
```

更完整的多阶段工具循环 smoke 见 `docs/CurrentState/AgentFramework.md`。
