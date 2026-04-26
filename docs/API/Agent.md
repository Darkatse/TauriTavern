# `window.__TAURITAVERN__.api.agent` — Agent API Draft

本文档是 Agent Host ABI 草案。它描述前端/扩展可见的稳定入口，不等同于 Rust 内部 service/repository。

状态：Phase 1 已实现最小骨架，本文仍包含后续阶段 API 草案。

## 1. 入口

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);

const agent = window.__TAURITAVERN__.api.agent;
```

Agent API 必须挂在 `window.__TAURITAVERN__.api.agent`。不要新增散落全局。

## 2. 方法概览

```ts
type TauriTavernAgentApi = {
  startRun(input: AgentStartRunInput): Promise<AgentRunHandle>;
  subscribe(runId: string, handler: (event: AgentRunEvent) => void, options?: AgentSubscribeOptions): Promise<TauriTavernHostUnsubscribe>;
  cancel(runId: string): Promise<void>;
  approveToolCall(input: AgentApproveToolCallInput): Promise<void>;
  listRuns(input: AgentListRunsInput): Promise<AgentRunSummary[]>;
  readEvents(input: AgentReadEventsInput): Promise<AgentReadEventsResult>;
  readWorkspaceFile(input: AgentReadWorkspaceFileInput): Promise<AgentWorkspaceFile>;
  readDiff(input: AgentReadDiffInput): Promise<AgentDiff>;
  rollback(input: AgentRollbackInput): Promise<AgentRollbackResult>;
  commit(input: AgentCommitInput): Promise<AgentCommitResult>;
};
```

`subscribe()` 返回的 unsubscribe 必须幂等。

## 3. startRun

```ts
type AgentStartRunInput = {
  chatRef: AgentChatRef;
  stableChatId?: string;
  generationType: 'normal' | 'regenerate' | 'swipe' | 'continue' | 'quiet' | 'impersonate';
  profileId?: string;
  promptSnapshot?: unknown;
  generationIntent?: AgentGenerationIntent;
  workspaceMode?: 'new-run' | 'resume-run';
  resumeRunId?: string;
  options?: {
    autoCommit?: boolean;
    stream?: boolean;
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

Public Host ABI 可以允许调用方省略 `stableChatId`，但 `api.agent.startRun()` 必须在调用 Rust command 前通过 `api.chat.open(chatRef).stableId()` 解析并校验。Rust command 不应自行读取 SillyTavern metadata。

短期允许 `promptSnapshot`，长期目标是 `generationIntent`。

要求：

- `stableChatId` 进入 backend DTO 前必须非空；无法解析时 fail-fast。
- `promptSnapshot` 与 `generationIntent` 至少提供一个。
- `resume-run` 必须提供 `resumeRunId`。
- `autoCommit` 默认由 profile/output policy 决定。
- 参数无效必须 reject，不静默回退 Legacy Generate。

## 4. subscribe

```ts
type AgentSubscribeOptions = {
  replay?: 'none' | 'from-seq';
  afterSeq?: number;
};
```

语义：

- 默认不复播全部历史。
- UI 首次打开 run 页面应先 `readEvents()`，再 `subscribe()`。
- 如果订阅期间漏事件，调用方可用 `afterSeq` 补拉。
- 底层 Tauri 事件名不是 Public Contract。

## 5. cancel

```ts
await agent.cancel(runId);
```

语义：

- 写 `run_cancel_requested`。
- 尽力取消当前模型请求或工具调用。
- Cancel 不是 failure。
- Cancel 后不能自动 commit。

## 6. approveToolCall

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

## 7. readEvents

```ts
type AgentReadEventsInput = {
  runId: string;
  afterSeq?: number;
  beforeSeq?: number;
  limit?: number;
};

type AgentReadEventsResult = {
  runId: string;
  events: AgentRunEvent[];
  hasMoreBefore: boolean;
  hasMoreAfter: boolean;
};
```

要求：

- `limit` 必须有上限。
- 移动端 UI 不应一次读取完整巨大 journal。

## 8. readWorkspaceFile

```ts
type AgentReadWorkspaceFileInput = {
  runId: string;
  path: string;
  checkpointId?: string;
};

type AgentWorkspaceFile = {
  runId: string;
  path: string;
  text?: string;
  bytes?: number;
  mime?: string;
  sha256?: string;
};
```

路径必须是 workspace relative path。非法路径直接 reject。

## 9. readDiff

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

## 10. rollback

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

## 11. commit

```ts
type AgentCommitInput = {
  runId: string;
  checkpointId?: string;
};

type AgentCommitResult = {
  runId: string;
  chatRef: AgentChatRef;
  stableChatId: string;
  messageIndex?: number;
  messageId?: string;
};
```

Commit 必须：

- 读取 manifest。
- 校验 required artifact。
- 校验当前 active chat 与 run 的 `stableChatId` 一致。
- 通过既有 chat 保存契约写入。
- 写 agent metadata。
- 追加 `run_committed` event。

## 12. Event Envelope

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

## 13. Errors

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

## 14. Rust Command 草案

```text
start_agent_run(dto, channel?)
cancel_agent_run(run_id)
approve_agent_tool_call(dto)
list_agent_runs(chat_ref)
read_agent_run_events(dto)
read_agent_workspace_file(dto)
read_agent_diff(dto)
rollback_agent_run(dto)
commit_agent_run(dto)
```

Command 层必须是薄封装。Agent loop 不写在 command 内。

## 15. Compatibility

Agent Mode off：

- `Generate()` 行为不变。
- `ToolManager` 行为不变。
- `api.chat` 行为不变。

Agent Mode on：

- 短期可使用 dryRun 生成 prompt snapshot。
- dryRun 不是纯函数，调用方必须理解它仍会触发上游事件。
- Agent tool loop 不递归 `Generate()`。
