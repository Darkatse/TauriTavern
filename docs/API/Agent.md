# Agent API

`window.__TAURITAVERN__.api.agent` 提供运行控制、历史读取和 Profile 管理。框架说明从 [Agent](../Agent/README.md) 开始，完整 TypeScript 类型见 [src/types.d.ts](../../src/types.d.ts)。

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

两种方法返回 `{ runId, status, workspaceId, stableChatId, generationType }`。准备好的 snapshot 需要包含 `contextPolicy` 与 `chatCompletionPayload`；独立预设和后续 Invocation 组装还会使用 `frozenRunInputSnapshot`。工具由 runtime 配置，输入消息应是尚未进入工具循环的初始提示词。组装过程见 [Prompt assembly](../Agent/PromptAssembly.md)。

## 控制与订阅

| 方法 | 行为 |
| --- | --- |
| `cancel(runId)` | 请求取消，返回 Run handle；终态通过事件观察 |
| `submitGuidance({ runId, text, clientGuidanceId? })` | 向活跃 Run 补充指令，返回 `guidanceId` 与 `status: 'queued'` |
| `subscribe(runId, handler, options?)` | 订阅持久事件，返回可重复调用的 unsubscribe |
| `subscribeLiveProjection(runId, handler, options?)` | 订阅当前工具参数预览，返回 unsubscribe |

补充指令在下一次前台模型请求前加入上下文，已经发出的请求保持原样。待处理指令若因 Run 结束而未使用，会记录丢弃事件。该接口不创建聊天消息。

`subscribe` 的选项是 `afterSeq`、`limit`、`intervalMs`、`onError`，默认从起点读取。实时预览只接受 `onError` 选项；它用于当前显示，历史过程从持久事件读取。

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

`beforeSeq` 读取该序号之前最近的一页，`afterSeq` 用于向前追新。`invocationId` 先筛选归属再分页，适合子 Agent 的局部 Timeline。`timelineProjection` 包含整个 Run 的 Invocation 和委派关系，独立于当前事件页。

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
| `profiles.retargetPresetRefs({ from, to })` | 预设重命名后更新引用；两端都是 `{ apiId, name }` |
| `promptAssembly.prepare(input)` | 解析 Profile 与冻结输入，返回组装模式及请求 |
| `promptAssembly.buildSnapshot(request)` | 在前端运行 PromptManager，生成 snapshot |
| `promptAssembly.buildCurrentModelConnectionSnapshot(input)` | 根据当前设置生成连接快照 |
| `promptAssembly.applyCurrentModelConnectionSnapshot(input)` | 把连接快照应用到组装设置 |
| `tools.list()` | 返回 `{ tools, diagnostics }`，包含内置工具和已发现的可用 MCP 工具 |

Profile 的用法见 [配置指南](../Agent/ProfilesAndPreset.md)。`tools.list()` 返回稳定工具 ID、描述和参数 schema；模型调用名称属于 Invocation 快照。

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

设置包括 `autoPruneEnabled`、`keepRecentTerminalRuns`、`keepFullRecentRuns`。完整保留数量不得大于核心历史数量；自动清理默认关闭。一次性 `retention` 参数只覆盖本次操作的保留数量。

预览区分缩减材料和整次删除，并返回不能清理的 Run。执行结果中的 `failedRuns` 表示未完成的项目。`detailLimit` 只控制返回明细数量，不改变执行范围或总计。

## 宿主生命周期

聊天提交由 runtime 与 host bridge 协作完成；模型用 `workspace.commit` 请求提交。宿主生成适配器使用 `settleChatPresentation(handle)` 等待该 handle 的聊天展示收尾。

聊天分叉时，`copyChatPersistentStates({ sourceChatRef, sourceStableChatId, targetChatRef, targetStableChatId })` 复制持久版本。删除消息或 swipe 后，`pruneChatPersistentStates({ chatRef?, stableChatId?, candidateStateIds })` 清理明确列出的、已不再被当前聊天引用的版本；该清理入口目前支持角色聊天。

`approveToolCall()` 是未实现的预留入口，调用会抛错。当前没有恢复 Run 或通用 rollback API。

## 错误与实现

异步方法失败时 reject；订阅错误通过 `onError` 传递。运行失败原因保存在日志，错误发生在已有确认提交之后时，Run 会保留输出并以 `partial_success` 结束。

- [agent.js](../../src/tauri/main/api/agent.js)：API 安装、启动与宿主桥接。
- [agent-run-runtime.js](../../src/tauri/main/api/agent-run-runtime.js)：控制、订阅和读取。
- [agent_commands.rs](../../src-tauri/crates/tauritavern/src/presentation/commands/agent_commands.rs)：Rust command 边界。
