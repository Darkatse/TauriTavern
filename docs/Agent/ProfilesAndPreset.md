# Profile 与预设

Profile 描述一个 Agent 如何工作：使用什么提示词和模型，能调用哪些工具，能读写哪些文件，以及如何交付结果。Preset 负责提示词的组织，LLM Connection 负责连接信息。

## 从默认配置开始

Agent System 的 Profile 面板可以复制和编辑配置。`Default Writer` 使用当前聊天的预设与模型，以 `output/main.md` 作为正文，支持工作区工具、Skill 和委派。内置默认配置和新建 Profile 默认开启流式传输；已保存配置继续使用原有设置。先用它完成一次运行，再按任务需要修改配置。

扩展也可以读取默认配置后另存一份：

```js
const agent = window.__TAURITAVERN__.api.agent;
const { profile } = await agent.profiles.load('default-writer');
const reviewer = structuredClone(profile);
reviewer.id = 'reviewer';
reviewer.displayName = 'Reviewer';
reviewer.instructions.agentSystemPrompt = '检查草稿中的人物动机，给出具体修改建议，并按任务要求返回结果。';
reviewer.delegation.callable = true;
reviewer.delegation.allowAsSubagent = true;
await agent.profiles.save(reviewer);
```

配置保存后，可通过 `profiles.diagnose('reviewer')` 查看模型、预设或工具引用的问题。

## 选择提示词和模型

两种绑定各自独立：

| 配置 | 选择 | 含义 |
| --- | --- | --- |
| `preset.mode` | `currentPromptSnapshot` | 使用当前生成准备好的提示词 |
| | `ref` | 用 `preset.ref` 指定的预设重新组装 |
| | `none` | 使用输入提示词，作为兼容绑定保留 |
| `model.mode` | `currentPromptSnapshot` | 使用本次输入冻结的模型连接 |
| | `connectionRef` | 使用指定 LLM Connection 和 `modelId` |
| | `requiresConfiguration` | 等待用户在本机选择模型 |

例如，使用专门的写作预设和本地连接：

```json
{
  "preset": {
    "mode": "ref",
    "ref": { "apiId": "openai", "name": "Writer Preset" }
  },
  "model": {
    "mode": "connectionRef",
    "connectionRef": "writer-model",
    "modelId": "model-name"
  }
}
```

这是 Profile 中的绑定片段。`writer-model` 需要是本机已经保存的连接。独立预设使用 OpenAI/chat-completion Preset，组装方式见 [Prompt assembly](PromptAssembly.md)。

`preset.reasoningEffort` 仅允许用于 `mode = ref`：省略时跟随预设，`auto` 不发送强度，其余档位覆盖预设。该字段不负责开启模型的思考模式。

Profile 面板中的 Model Target 会物化为 LLM Connection。连接的端点和凭据更新供后续解析使用，Profile 的 `modelId` 则保留用户选定的值。导出或嵌入到角色卡、预设时，本机的独立模型绑定会改为 `requiresConfiguration`，导入后重新选择即可。

## 调整工作方式

| 字段 | 用途 |
| --- | --- |
| `instructions.agentSystemPrompt` | Agent 的工作指令；省略时使用默认指令 |
| `context.initialChatHistoryMessages` | 初始历史楼数：`-1` 不主动裁剪，`0` 不注入，正数取最近 N 楼；仍受模型上下文预算限制 |
| `context.includeActivatedWorldInfo` | 是否在初始提示词中包含已激活世界书 |
| `tools.allow` / `deny` | 可用工具，deny 优先 |
| `tools.maxRounds` / `maxCallsPerRun` / `maxCallsPerTool` | 每个 Invocation 的轮数与调用预算 |
| `tools.externalResultInlineCharLimit` | MCP 与 Extension 结果的模型内联字符阈值，默认 50,000；长结果保留完整可读文件 |
| `tools.toolDescriptions` | 替换模型看到的工具或参数描述 |
| `skills.visible` / `deny` | 按名称选择可用 Skill |
| `workspace.visibleRoots` / `writableRoots` | 文件读写范围 |
| `run.presentation` | 前台聊天输出或后台文件处理 |
| `run.stream` / `modelRetry` | 流式预览与模型请求重试 |
| `run.directRunnable` | 是否出现在直接运行的 Agent 选择列表 |
| `output.artifacts` | 输出文件；当前正文目标为 `messageBody` |

工具字段使用 [稳定 ID](ToolSystem.md#身份与调用名称)。新启用未加载的扩展工具会拒绝本次修改；已有选择暂时缺席时保留配置，运行时排除该项并记录诊断，不阻塞其他编辑与运行。Chat 和共享 Session Profile 遵循同一规则。

`plan/` 可以保存普通计划文件。当前运行器支持的 Profile plan 配置是 `mode: "none"`、空 `nodes`，尚无节点式工作流执行器。

## 配置协作

调用方开启 `delegation.canDelegate` 或 `canHandoff`，并允许相应工具。接收方开启 `callable`，再按用途选择 `allowAsSubagent` 或 `allowAsHandoffTarget`；`allowedCallers` 指定可调用它的 Profile，`["*"]` 表示所有调用方。

`descriptionForAgents` 说明适合处理的工作，未填写时使用 `description`。并发、任务数量和交接深度也在 `delegation` 中设置。完整流程见 [多 Agent 协作](SubAgent.md)。

## Session 配置

Session 使用相同的 `AgentProfileDefinition`，独立保存到 `_tauritavern/agent-workspaces/sessions/profile.json`，由所有 Session 共用，不进入普通 Profile 列表。修改从下一次发送准备时生效；预设重命名同时更新其引用。

Session 必须指定 `preset.mode = ref` 和 `model.mode = connectionRef`，Skill 只取 Profile 自身作用域。正文产物与 commit/finish 要求属于 Chat 执行准入，Session 可使用空产物配置。目录与生命周期见 [Workspace](Workspace.md#session-的持续工作区)。

## 源码

普通 Profile 以 JSON 保存到 `_tauritavern/agent-profiles/profiles/`，当前 schema 版本为 4。

schema 1–3 在加载或导入时自动迁移，移除旧 `skill.*`、`agent.list` 工具配置及 Skill 读取预算；保留其余配置，不自动授予新权限或重写指令。

旧 `tools.mcpResultInlineCharLimit` 的数值在读取时保留，再次保存使用 `tools.externalResultInlineCharLimit`。

- [profile.rs](../../src-tauri/crates/tt-domain/src/models/agent/profile.rs)：字段与默认值。
- [agent_profile_service](../../src-tauri/crates/tt-application/src/services/agent_profile_service)：默认配置、解析和验证。
- [profile-model.ts](../../src/scripts/extensions/agent-system/src/profile-model.ts)：Profile 编辑与可移植配置。
- [LLM Connection API](../API/LlmConnections.md)：连接的创建与管理。
