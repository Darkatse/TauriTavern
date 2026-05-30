# `window.__TAURITAVERN__.api.agent` — Agent API

本文档是 Agent Host ABI 当前参考。它描述前端/扩展可见的稳定入口，不等同于 Rust 内部 service/repository。

状态：当前已实现 canonical model IR、provider native metadata 保真、provider_state continuation、上下文只读工具、Skill tools、workspace 读改工具循环、前端 dryRun adapter，以及 Agent Profile 独立 preset / 独立 model 的 Frontend PromptAssemblyBroker 链路。Agent System 扩展开关开启时，普通发送、`/trigger`、regenerate 与 overswipe 新候选生成会通过 Legacy Generate 兼容桥启动 Agent；普通切换已有 swipe 候选不启动 Agent。本文以当前已落地 Host ABI 为准，并在后续章节保留 readDiff/rollback/approval/listRuns 等未来设计。

`provider_state` 是 Rust 后端内部 continuation contract，不是 Host ABI。前端/扩展不应读写 `_tauritavern_provider_state`；需要诊断时通过 run events、`modelResponsePath` 与 LLM API log 观察。
模型回合详情必须通过 `readModelTurn()` 读取后端投影 DTO；前端不解析 `model-responses/` raw JSON。

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
  readModelTurn(input: AgentReadModelTurnInput): Promise<AgentModelTurn>;
  profiles: {
    list(): Promise<{ profiles: AgentProfileSummary[] }>;
    load(input: string | { profileId: string }): Promise<{ profile: AgentProfileDefinition | null }>;
    resolveSystemPrompt(input?: string | { profileId?: string | null }): Promise<{ agentSystemPrompt: string }>;
    save(input: AgentProfileDefinition | { profile: AgentProfileDefinition }): Promise<void>;
    delete(input: string | { profileId: string }): Promise<void>;
  };
  promptAssembly: {
    prepare(input: AgentPromptAssemblyPrepareInput): Promise<AgentPromptAssemblyPrepareResult>;
    buildSnapshot(input: AgentPromptAssemblyBrokerRequest): Promise<AgentPromptAssemblyBuildResult>;
  };
  tools: {
    list(): Promise<{ tools: AgentToolSpec[] }>;
  };

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
  presentation?: 'foreground' | 'background';
  options?: {
    presentation?: 'foreground' | 'background';
    stream?: false;
  };
};
```

`startRunFromLegacyGenerate()` 是当前前端兼容桥：它使用 Legacy `Generate(..., dryRun = true)` 捕获当前 SillyTavern prompt 输入语义与 `FrozenRunInputSnapshot`。若 Profile 使用独立 preset，则先通过 `promptAssembly.prepare()` / `buildSnapshot()` 复用真实 PromptManager 组装；否则物化当前 prompt snapshot，再调用 `startRunWithPromptSnapshot()`。

要求：

- 只用于当前 active chat。
- 当前只支持 `main_api = openai` 的 chat-completion 路径。
- 必须禁用 Legacy ToolManager tools；Agent tools 只能由 Rust runtime 注册。
- `worldInfoActivation` 必须来自本次 dryRun 的最终 `WORLDINFO_SCAN_DONE`，不能读取全局 last activation 当作 run 真相。
- `stream` 必须为 `false` 或省略；`presentation` 可显式覆盖 profile 默认前台/后台语义。
- dryRun 没有产出 messages、已有 tool turns、已有 external tools 都必须 reject，不回退 Legacy Generate。
- 入口路由到 Agent 后，profile、provider、group chat、context policy 或 Host API 错误必须 reject 并显式呈现；不得静默降级为 Legacy Generate。`/trigger` 作为生成入口遵守同一规则。

注意：`Generate(..., dryRun = true)` 不返回 payload。它只 emit `GENERATE_AFTER_DATA`，然后 resolve `undefined`。调用方不应写 `const payload = await Generate(..., true)`；捕获逻辑由 `startRunFromLegacyGenerate()` 内部 adapter 负责。

## 4. startRunWithPromptSnapshot

```ts
type AgentStartRunWithPromptSnapshotInput = {
  chatRef: AgentChatRef;
  stableChatId?: string;
  generationType?: 'normal' | 'regenerate' | 'swipe' | 'continue' | 'quiet' | 'impersonate';
  profileId?: string;
  promptSnapshot: AgentPromptSnapshot;
  frozenRunInputSnapshot?: unknown;
  generationIntent?: AgentGenerationIntent;
  workspaceMode?: 'new-run' | 'resume-run';
  resumeRunId?: string;
  presentation?: 'foreground' | 'background';
  options?: {
    presentation?: 'foreground' | 'background';
    stream?: boolean;
  };
};

type AgentPromptSnapshot = {
  contextPolicy: unknown;
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
  generationType: string;
};
```

身份语义：

- `stableChatId` 是聊天的长期稳定身份。
- `workspaceId` 必须由 `kind + stableChatId` 派生，不得由可变的 `chatRef` 文件名直接决定。
- `runId` 是一次 Agent 执行身份，每次 normal/regenerate/swipe/continue 都必须生成新的 `runId`。
- `generationType` 是启动时已校验的 SillyTavern generation intent；前端 retry 等宿主行为必须使用它或 `generation_intent_recorded` 事件，不得从 DOM 状态猜测。
- 同一稳定聊天的多次 run 应共享同一个 chat workspace，但各自拥有独立 run workspace。

Public Host ABI 可以允许调用方省略 `stableChatId`，但 `api.agent.startRunWithPromptSnapshot()` 必须在调用 Rust command 前通过 `api.chat.open(chatRef).stableId()` 解析并校验。Rust command 不应自行读取 SillyTavern metadata。

当前要求提供 `promptSnapshot.contextPolicy` 与 `promptSnapshot.chatCompletionPayload`。`promptSnapshot.worldInfoActivation` 是可选字段，由 `worldinfo_read_activated` 读取；`frozenRunInputSnapshot` 用于审计与后续独立 prompt assembly 复用。长期目标是 `generationIntent + ContextFrame`，但当前 Rust runtime 不会只凭 `generationIntent` 组装上下文。

要求：

- `stableChatId` 进入 backend DTO 前必须非空；无法解析时 fail-fast。
- `promptSnapshot.chatCompletionPayload` 必须包含 chat-completion payload object。
- 如果调用方希望 `worldinfo_read_activated` 返回非错误结果，必须在 prompt snapshot 中提供本次 run 的 `worldInfoActivation`。
- 当前拒绝已有 `tools`、`tool_choice`、`role: "tool"` 或已有 `tool_calls` 的外部 tool turns。
- 当前拒绝 `stream: true`。
- `workspaceMode` / `resumeRunId` 当前只是后续字段，不应作为当前行为依赖。
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
  chars: number;
  words: number;
  sha256: string;
};
```

路径必须是 workspace relative path。非法路径直接 reject。
当前 Host ABI 只读当前 run workspace 的 UTF-8 文本文件，不支持 `checkpointId` 参数。模型侧读取应使用 `workspace_read_file` 工具，前端/扩展侧读取应使用本方法。

## 10. readModelTurn

```ts
type AgentReadModelTurnInput = {
  runId: string;
  invocationId?: string;
  round: number;
  maxChars?: number;
};

type AgentModelTurn = {
  runId: string;
  round: number;
  modelResponsePath: string;
  provider: {
    source?: string;
    format?: string;
    model?: string;
    responseId?: string;
    usage?: unknown;
  };
  assistant: {
    text: string;
    totalChars: number;
    totalWords: number;
    truncated: boolean;
  };
  reasoning: Array<{
    source: string;
    text: string;
    totalChars: number;
    totalWords: number;
    truncated: boolean;
  }>;
  toolCalls: Array<{
    callId: string;
    name: string;
    modelName?: string;
  }>;
};
```

`assistant.text` 与 `reasoning[].text` 会按 `maxChars` 截断；`totalChars` / `totalWords` 始终表示截断前完整文本的字词统计。

`round` 必须大于 0。`maxChars` 省略时由后端使用默认上限；传入时必须大于 0。
`invocationId` 省略时读取 root invocation；读取 SubAgent / handoff invocation 的模型回合时必须传入对应 invocation id。

该方法返回面向 UI 的白名单投影：assistant 输出、可见/摘要化 reasoning、工具调用摘要与 provider 摘要。它不会暴露完整 raw response、provider-private native continuation、签名或 encrypted reasoning。需要完整诊断时仍使用 run workspace 中的 `modelResponsePath` 与 LLM API log。

## 11. profiles / promptAssembly / tools

`profiles.*` 是当前 Agent Profile 管理入口。`profiles.list()` 的 summary 包含 `directRunnable`，供前端区分可直接启动的 root-run Profile 与只能作为 SubAgent / handoff target 的 Profile。Profile JSON 中的 `preset.mode = "ref"` 与 `model.mode = "connectionRef"` 会影响 prompt assembly 和最终模型连接；`model.mode = "requiresConfiguration"` 表示 Profile 需要本机重新选择模型，可保存但不可运行；`run.directRunnable = false` 表示该 Profile 不能直接启动，只能通过已实现的非直接入口运行（当前为 return-mode SubAgent）。前端“可作为子 Agent”会写入该非直接运行语义。保存时无效 schema 必须 fail-fast。

```ts
type AgentProfileSummary = {
  id: string;
  displayName: string;
  description?: string;
  directRunnable: boolean;
};
```

`promptAssembly.prepare()` 调用 Rust `prepare_agent_prompt_assembly`，返回 `currentPromptSnapshot` 或 `frontendPromptAssembly`。`promptAssembly.buildSnapshot()` 是前端 broker：它只能使用 `frozenRunInputSnapshot` 内的 `promptInputs`、`worldInfoActivation`、`macroContext`，并调用真实 SillyTavern PromptManager 组装 `promptSnapshot.chatCompletionPayload`。该 API 是 Agent orchestration 内部边界，不是第三方扩展任意改写 prompt 的入口。

### tools.list

```ts
type AgentToolSpec = {
  name: string;
  modelName: string;
  title: string;
  description: string;
  inputSchema: unknown;
  outputSchema?: unknown;
  annotations?: unknown;
  source: string;
};
```

`tools.list()` 返回当前后端 Agent Tool Registry 的 canonical specs。Profile 面板可以用它编辑 `tools.toolDescriptions`，但不得把返回值当作可修改的 registry。

## 12. readDiff

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

## 13. rollback

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

## 14. commit

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

Chat commit 不是公开 Host API 方法，而是 Agent tool 与 host bridge 的内部握手：

- 模型调用 `workspace.commit`，无参数时默认 `replace output/main.md`。
- Rust runtime 读取 workspace 文件、校验 required message body、创建 checkpoint，并写 `chat_commit_requested` event。
- 前端 host bridge 校验当前 active chat 与 run 的 `chatRef/stableChatId` 一致。
- bridge 通过上游 `saveReply()` 写入聊天，再调用 `resolve_agent_chat_commit`。
- `chat_commit_requested` 不携带 `persistStateId`；该字段只能在 `workspace.finish` 成功提交 persistent state 后，由 `persistent_state_metadata_update_requested` / `resolve_agent_persistent_state_metadata_update` 写回同一条 chat message。
- `replace` 后续使用 `appendFinal` 覆盖同一消息；`append` 后续使用 `append` 追加同一消息。
- `append` 在本 run 尚无 commit 时不会报错，会创建本 run 的消息楼层。
- 前台 run 在 `workspace.finish` 前必须至少成功 commit 一次；后台 run 可无 chat commit 完成。

## 15. Event Envelope

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

## 16. Errors

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

## 17. Rust Command

```text
start_agent_run(dto)
list_agent_tool_specs()
cancel_agent_run(dto)
read_agent_run_events(dto)
read_agent_workspace_file(dto)
resolve_agent_chat_commit(dto)
```

Command 层必须是薄封装。Agent loop 不写在 command 内。

后续命令：

```text
approve_agent_tool_call(dto)
list_agent_runs(chat_ref)
read_agent_diff(dto)
rollback_agent_run(dto)
```

## 18. Compatibility

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

当前开放十四个非 delegation 内建工具：

| Canonical name | Model-facing alias | 说明 |
| --- | --- | --- |
| `chat.search` | `chat_search` | 搜索当前 run 绑定的聊天。只有 `query` 必填；可选 `limit`、`role`、`start_message`、`end_message`、`scan_limit`。返回 message index、snippet 与 ref。 |
| `chat.read_messages` | `chat_read_messages` | 按 0-based message index 读取当前聊天消息；每项可选 `start_char`、`max_chars` 读取长消息片段。 |
| `worldinfo.read_activated` | `worldinfo_read_activated` | 读取本次 run 的最终激活世界书条目；模型可读文本只包含条目名、世界书名、条目内容。 |
| `dice.roll` | `dice_roll` | 为明确的随机、跑团或 roleplay 检定投骰；支持 `d6`、`1d20`、`3d6+4` 与纯数字。默认 Profile 不启用。 |
| `skill.list` | `skill_list` | 列出当前 Profile 可见的已安装 Skill 索引摘要。 |
| `skill.search` | `skill_search` | 搜索当前 Profile 可见的单个 Skill 内 UTF-8 文本文件，返回 snippet/ref。 |
| `skill.read` | `skill_read` | 读取已安装 Skill 内的 UTF-8 文本文件或范围；默认 `SKILL.md`，支持 `path`、行范围、字符范围与 `max_chars`。 |
| `workspace.list_files` | `workspace_list_files` | 列出模型可见 workspace 文件；`path` 省略、空字符串、`.`、`./` 表示 workspace root |
| `workspace.search_files` | `workspace_search_files` | 搜索模型可见 workspace UTF-8 文本文件，返回 snippet/ref |
| `workspace.read_file` | `workspace_read_file` | 读取 UTF-8 文本文件并返回行号；支持行范围和字符范围；完整读取记录 read-state |
| `workspace.write_file` | `workspace_write_file` | 写 UTF-8 文本到 manifest 可写 roots，当前为 `output/`、`scratch/`、`plan/`、`summaries/`、`persist/` |
| `workspace.apply_patch` | `workspace_apply_patch` | 单文件 `old_string` / `new_string` 精确替换，要求已完整读取或由本 run 创建/修改 |
| `workspace.commit` | `workspace_commit` | 提交可见 workspace 文件到当前聊天；无参数默认 replace `output/main.md`；append 首次创建、后续追加同一消息 |
| `workspace.finish` | `workspace_finish` | 结束工具循环；前台 run 要求已有成功 commit，后台 run 可直接结束 |

当前不存在 MCP、shell 或 extension bridge 工具。

模型可修正的工具错误会作为 `is_error = true` tool result 回填下一轮。宿主级 IO、journal、checkpoint、序列化、取消和模型响应结构错误仍然让 run failed。

推荐最小启动：

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);

const agent = window.__TAURITAVERN__.api.agent;

const run = await agent.startRunFromLegacyGenerate({
  generationType: 'normal',
  options: { stream: false, presentation: 'foreground' },
});

const stop = agent.subscribe(run.runId, event => console.log(event));
```

更完整的多轮工具循环 smoke 见 `docs/CurrentState/AgentFramework.md`。
