# Agent API

`window.__TAURITAVERN__.api.agent` 提供 Chat 与 Session 运行、历史读取和 Profile 管理。框架说明从 [Agent](../Agent/README.md) 开始，完整 TypeScript 类型见 [src/types.d.ts](../../src/types.d.ts)。

## 启动一次运行

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const agent = window.__TAURITAVERN__.api.agent;

const run = await agent.startRunFromLegacyGenerate({ profileId: 'default-writer' });
const unsubscribe = agent.subscribe(run.runId, event => {
  console.log(event.seq, event.type, event.payload);
}, { onError: console.error });

// 离开当前视图时调用 unsubscribe()。
```

该入口从当前聊天捕获生成输入，适用于 OpenAI/chat-completion 路径。普通发送、`/trigger`、regenerate 和生成新 swipe 在 Agent Mode 开启时使用同一套运行机制。

| 启动方法 | 输入 |
| --- | --- |
| `startRunFromLegacyGenerate(input?)` | 当前聊天与 Legacy Generate 选项 |
| `startRunWithPromptSnapshot(input)` | 调用方已经组装好的 `promptSnapshot` 与 `chatRef` |

共同选项有 `profileId`、`generationType`、`stableChatId`、`presentation` 和 `options.stream`。稳定聊天 ID 省略时由 Host API 解析。`options.stream` 省略时使用各 Invocation 的 Profile 设置，显式值覆盖整次 Run。

两种 Chat 方法返回 `{ runId, status, workspaceId, stableChatId, generationType }`。Snapshot 使用 `{ contextPolicy, messages, generationParameters }`，消息为 `AgentModelMessage[]`；旧 `chatCompletionPayload` 在入口适配，Chat 输入不能注入外部工具回合。冻结输入与组装规则见 [Prompt assembly](../Agent/PromptAssembly.md)。

canonical snapshot 的指令消息以 Text part 保存正文，并携带 `providerMetadata.promptComponent: "agentSystemPrompt"`，供 runtime 追加目录。旧 Chat 输入仍接受字符串 `content` 与 `_tauritavern_prompt_component: "agentSystemPrompt"`。共同 PromptManager 自动提供标记，标记不会发送给模型。

## 持续 Session

Session 独立于角色聊天和写作 Agent Mode。使用前通过 `sessions.profile.save()` 保存[共享配置](../Agent/ProfilesAndPreset.md#session-配置)，此后每次发送读取最新配置。

| 方法 | 返回内容 |
| --- | --- |
| `sessions.profile.load()` | `{ profile }`；尚未配置时为 `{ profile: null }` |
| `sessions.profile.save(profile)` | 保存共享的 `AgentProfileDefinition` |
| `sessions.create()` | `{ session }`；元数据为 `{ id, createdAt, title, lastUsedAt }` |
| `sessions.list()` | `{ sessions, activeRuns }`；仅会话元数据目录与活动 Run handles |
| `sessions.rename({ sessionId, title })` | `{ session }`；标题 trim 后为 1–120 字符 |
| `sessions.delete({ sessionId })` | 删除会话数据；活动会话拒绝删除，目标已不存在时成功 |
| `sessions.read({ sessionId, beforeSeq?, limit? })` | `{ session, messages, lastSeq, nextBeforeSeq, activeRun }` |
| `sessions.send({ sessionId, text })` | `{ sessionId, runId, status }`；执行在原生端继续 |

```js
// 已配置 Session Profile；agent 的取得方式同上。
const { session } = await agent.sessions.create();
const run = await agent.sessions.send({ sessionId: session.id, text: '查看 work/ 中的文件。' });
```

保存 `session.id` 供后续读取和发送。`send` 返回后执行仍在继续，通过 `subscribe(run.runId, ...)` 观察终态，再发送下一条。忙碌或准备输入过期时 reject，不自动重试。

`title`、`lastUsedAt` 未设置或旧数据缺字段时为 null。用户消息落盘时更新 `lastUsedAt`，未命名时生成短标题。目录按 `lastUsedAt ?? createdAt` 降序排列；改名不改变排序。

历史由后端保存；收到 `session_message_appended` 后可调用 `sessions.read()` 更新界面，实时进度沿用 `subscribeLiveProjection()`。历史页按 seq 升序返回，每项为 `{ seq, runId, createdAt, message, origin? }`；`nextBeforeSeq` 用于向前翻页，`activeRun` 为活动 handle 或 null。

`origin: { invocationId, round }` 关联 assistant/tool 记录所属的模型回合，不进入模型消息；user 消息与旧历史可不带 origin。Session seq 标识持久消息。

Session 的结束与恢复边界见 [运行循环](../Agent/Runtime.md)，文件保留规则见 [Workspace](../Agent/Workspace.md#session-的持续工作区)。

## 控制与订阅

| 方法 | 行为 |
| --- | --- |
| `cancel(runId)` | 请求取消，返回 Chat handle 或 `{ sessionId, runId, status }`；终态通过事件观察 |
| `readCheckpoint(runId)` | 读取保存状态及可恢复性信息 |
| `resume({ runId, additionalRounds?, revisionGuidance? })` | 续接同一 Run，或根据新要求修订已完成输出；返回含订阅游标 `afterSeq` 的 Run handle |
| `submitGuidance({ runId, text, clientGuidanceId? })` | 向活跃 Run 补充指令，返回 `guidanceId` 与 `status: 'queued'` |
| `subscribe(runId, handler, options?)` | 订阅持久事件，返回可重复调用的 unsubscribe |
| `subscribeLiveProjection(runId, handler, options?)` | 订阅当前正文、推理文字及文件工具参数预览，返回 unsubscribe |

Chat 补充指令在下一次前台模型请求前加入上下文，已经发出的请求保持原样。尚未消费的指令随续接保留。该接口不创建聊天消息。

`subscribe` 的选项是 `afterSeq`、`limit`、`intervalMs`、`onError`，默认从起点读取。实时预览只接受 `onError`；历史过程从持久事件读取。退订后不再交付回调，包括在途请求的结果与错误。

实时通道使用以下共同协议（不写入持久 journal）：

| update.type | 内容 |
| --- | --- |
| `snapshot` | `{ calls, responses }`，替换当前投影 |
| `responseReplace` | `{ response }`，新的模型回合或重试 |
| `responseAppend` | `{ invocationId, text, reasoning, toolIds }`，三个字段分别追加 |
| `responseRemove` | `{ invocationId }`，移除本次临时响应 |
| `replace` / `append` / `remove` | 既有文件工具参数投影 |

`response` 包含 `invocationId`、`invocationExitPolicy`、`round`、`attempt`、`text`、`reasoning`、`toolIds`。正文与思考共用回合/尝试身份，完成快照不重复发送增量。正式 assistant 按 `runId + invocationId + round` 接替对应 attempt 的预览，须处理事件与 Channel 的任意到达顺序。失败、取消及终态清除预览，不将其写入历史。写作 Timeline 只消费 reasoning，Chat 正文仍走文件提交。

`resume` 保留原始输入与累计预算，要求当前聊天及消息仍属于原 Run；普通重新生成仍创建新 Run。轮数不足时可经用户明确选择追加 `additionalRounds`。续接订阅使用返回的 `afterSeq`，恢复条件见 [运行循环](../Agent/Runtime.md#checkpoint-与恢复)。

传入 `revisionGuidance` 时，从已完成的 checkpoint 开始新的前台 Invocation，继承上下文并使用原 Profile 的预算。聊天命令 `/fix 修改要求` 使用这一入口，原地修改最后一条 Agent 回复的当前 swipe，以当前已保存的正文为基准。

v1 checkpoint 仅支持查看状态和修订已完成的 Run；首次修订时转换，范围见 [运行循环](../Agent/Runtime.md#checkpoint-与恢复)。

## 历史与详情

```js
const page = await agent.listRuns({ stableChatId, limit: 20 });
const nextPage = page.nextCursor
  ? await agent.listRuns({ stableChatId, before: page.nextCursor, limit: 20 })
  : null;

const { events, timelineProjection } = await agent.readEvents({
  runId,
  afterSeq: 0,
  limit: 100,
  includeTimelineProjection: true,
});
```

| 方法 | 返回内容 |
| --- | --- |
| `listRuns({ chatRef?, stableChatId?, statuses?, before?, limit? })` | `{ runs, nextCursor? }`，按创建时间倒序；`before` 为 `{ createdAt, runId }`，最多 200 条 |
| `readEvents({ runId, afterSeq?, beforeSeq?, limit?, invocationId?, includeTimelineProjection? })` | `{ events, timelineProjection? }`，事件按序号升序，单页最多 500 条 |
| `readWorkspaceFile({ runId, path })` | `{ path, text, chars, words, sha256 }` |
| `readModelTurn({ runId, invocationId?, round, maxChars? })` | assistant 文本、可见 reasoning、工具调用与 provider 摘要 |
| `readTaskDetail({ runId, taskId, includeResult? })` | 任务说明、当前状态及按需读取的结果 |

`listRuns` 保持 Chat 历史范围；Session 对话使用 `sessions.read`，按 runId 的事件、模型回合及工作区详情读取可用于两种 Run。`beforeSeq` 读取该序号之前最近的一页，`afterSeq` 用于向前追新。`invocationId` 先筛选归属再分页，适合子 Agent 的局部 Timeline。`timelineProjection` 包含整个 Run 的 Invocation 和委派关系，独立于当前事件页。

`readModelTurn` 的 `round` 从 1 开始，省略 `invocationId` 时读取根 Agent。`maxChars` 限制展示文本，字词总数仍对应完整内容。

`readTaskDetail` 把委派的 `task` 和交接的 `handoff` 统一放在返回值的 `task` 字段，保留自定义字段。状态是读取时的状态。`includeResult` 默认 `false`；设为 `true` 时，若有结果则返回 `{ summary, summaryRef, output }`，否则 `result` 为 `null`。

模型回合、任务详情、工具目录与 Timeline 关系属于随项目演进的 UI 投影（Project Contract）。界面通过这些 API 读取内容，具体类型集中在 `src/types.d.ts`。日志语义见 [运行日志](../Agent/RunEventJournal.md)。

## Profile、提示词与工具目录

| 方法 | 用途 |
| --- | --- |
| `profiles.list()` | 返回 Profile 列表及文件诊断 |
| `profiles.load(profileId)` | 返回 `{ profile }` |
| `profiles.save(profile)` / `delete(profileId)` | 保存或删除配置 |
| `profiles.diagnose(profileId)` | 检查预设、模型、工具等引用 |
| `profiles.resolveSystemPrompt({ profileId? })` | 返回 `{ agentSystemPrompt }` |
| `profiles.repairFile({ profileId, action })` | 删除损坏配置或规范化身份；action 为 `delete`、`normalizeIdentity` |
| `profiles.retargetPresetRefs({ from, to })` | 预设重命名后更新普通 Profile 和共享 Session Profile 的匹配引用；两端都是 `{ apiId, name }` |
| `promptAssembly.prepare(input)` | 解析 Profile 与冻结输入，返回组装模式及请求 |
| `promptAssembly.buildSnapshot(request)` | 在前端运行 PromptManager，生成 snapshot |
| `promptAssembly.buildCurrentModelConnectionSnapshot(input)` | 根据当前设置生成连接快照 |
| `promptAssembly.applyCurrentModelConnectionSnapshot(input)` | 把连接快照应用到组装设置 |
| `tools.list({ context? }?)` | 返回 `{ tools, diagnostics }`，包含内置、可用 MCP 及已注册扩展工具；context 可为 `chat` 或 `session` |
| `tools.register(definition, execute)` | 注册普通 JS 工具函数，返回 `Promise<void>` |
| `tools.setEnabled(toolId, enabled)` | 启用或关闭已注册的扩展工具，不删除函数或改写 Profile |

Profile 的用法见 [配置指南](../Agent/ProfilesAndPreset.md)。`tools.list()` 返回稳定工具 ID、描述和参数 schema；模型调用名称属于 Invocation 快照。

`profiles.retargetPresetRefs` 返回 `{ updated, profileIds, sessionProfileUpdated }`；总数 `updated` 包含共享配置，`profileIds` 只列普通 Profile。

`profiles.resolveSystemPrompt` 返回 Profile 指令正文；运行时目录的追加规则见 [Prompt assembly](../Agent/PromptAssembly.md#skill-与-agent-目录)。

## 注册扩展工具

在扩展启动入口注册普通 JS 函数，可直接使用模块引用和闭包。稳定 ID 为 `extension/<extensionId>:<name>`，重复注册报错；注册持续到主页面 reload，与面板开关无关。

```js
await agent.tools.register({
    extensionId: 'my-extension',
    name: 'read_auto_reply_settings',
    description: '读取自动回复的启用状态。',
    inputSchema: { type: 'object', properties: {} },
    contexts: ['chat', 'session'],
    enabled: true,
}, async () => {
    return { enabled: true };
});

await agent.tools.setEnabled('extension/my-extension:read_auto_reply_settings', false);
```

模型以 `name` 为调用名称基础，命名与结果表达遵循 [工具契约](../Agent/ToolSystem.md)。`description` 说明用途与必要限制，返回值和错误说明实际结果或具体问题。

`contexts` 必填且非空，`enabled` 默认 true；注册后仍需在 [Profile](../Agent/ProfilesAndPreset.md#调整工作方式) 中选择工具。关闭后不再接受新调用，已开始的调用继续执行，`list()` 仍包含关闭条目。开关持久化由扩展自己的设置或 `api.extension.store` 负责，页面 reload 后按保存的值重新注册。

回调接收 JSON 对象参数与执行上下文，其中 `target` 指向本次 Run 所属的 Chat 或 Session，与当前 UI 无关；`signal: AbortSignal` 提供合作式取消，不回滚已发生的操作。完整字段见 [类型定义](../../src/types.d.ts)。

函数可异步返回 JSON 值，顶层 `undefined` 按 `null` 返回；抛错或无法通过 JSON IPC 传输时报告工具错误。等待可取消，60 秒无回执则结束本次运行，不自动重发。

## 保留策略

```js
const settings = await agent.retention.readSettings();
const plan = await agent.retention.planPrune({ detailLimit: 20 });
```

| 方法 | 行为 |
| --- | --- |
| `retention.readSettings()` | 返回自动清理开关及两个保留数量 |
| `retention.updateSettings(patch)` | 保存部分设置 |
| `retention.planPrune({ retention?, detailLimit? })` | 预览清理范围和统计 |
| `retention.applyPrune({ retention?, detailLimit? })` | 重新计算并执行清理，返回结果和 `afterPlan` |

此清理范围为 Chat Run，不包含 Session。设置包括 `autoPruneEnabled`、`keepRecentTerminalRuns`、`keepFullRecentRuns`。完整保留数量不得大于核心历史数量；自动清理默认关闭。一次性 `retention` 参数只覆盖本次操作的保留数量。

预览区分缩减材料和整次删除，并返回不能清理的 Run。执行结果中的 `failedRuns` 表示未完成的项目。`detailLimit` 只控制返回明细数量，不改变执行范围或总计。

## 宿主生命周期

聊天提交由 runtime 与宿主协作完成；模型用 `workspace.commit` 请求提交。宿主调用 `settleChatPresentation({ runId })` 等待消息呈现与 checkpoint 保存；失败后可重试，已完成的消息写入不会重复执行。

聊天分叉时，`copyChatPersistentStates({ sourceChatRef, sourceStableChatId, targetChatRef, targetStableChatId })` 复制持久版本。删除消息或 swipe 后，`pruneChatPersistentStates({ chatRef?, stableChatId?, candidateStateIds })` 清理明确列出的、已不再被当前聊天引用的版本；该清理入口目前支持角色聊天。

`approveToolCall()` 是未实现的预留入口，调用会抛错。当前没有通用 rollback API。

## 错误与实现

异步方法失败时 reject；订阅错误通过 `onError` 传递。运行失败原因保存在日志，错误发生在已有确认提交之后时，Run 会保留输出并以 `partial_success` 结束。

- [types.d.ts](../../src/types.d.ts)：接口参数与返回类型。
- [agent.js](../../src/tauri/main/api/agent.js)：API 安装、启动与宿主桥接。
- [agent-run-runtime.js](../../src/tauri/main/api/agent-run-runtime.js)：控制、订阅和读取。
- [agent-sessions.js](../../src/tauri/main/api/agent-sessions.js)：Session SDK 与共享 Profile。
- [agent_commands.rs](../../src-tauri/crates/tauritavern/src/presentation/commands/agent_commands.rs)：Rust command 边界。
