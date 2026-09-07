# Profile 与预设

Profile 描述一个 Agent 如何工作：使用什么提示词和模型，能调用哪些工具，能读写哪些文件，以及如何交付结果。Preset 负责提示词的组织，LLM Connection 负责连接信息。

## 从默认配置开始

Agent System 的 Profile 面板可以复制和编辑配置。`Default Writer` 使用当前聊天的预设与模型，以 `output/main.md` 作为正文，支持工作区工具、Skill 和委派。先用它完成一次运行，再按任务需要修改配置。

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

Profile 面板中的 Model Target 会物化为 LLM Connection。连接的端点和凭据更新供后续解析使用，Profile 的 `modelId` 则保留用户选定的值。导出或嵌入到角色卡、预设时，本机的独立模型绑定会改为 `requiresConfiguration`，导入后重新选择即可。

## 调整工作方式

| 字段 | 用途 |
| --- | --- |
| `instructions.agentSystemPrompt` | Agent 的工作指令；省略时使用默认指令 |
| `context.initialChatHistoryMessages` | 初始历史楼数：`-1` 不主动裁剪，`0` 不注入，正数取最近 N 楼；仍受模型上下文预算限制 |
| `context.includeActivatedWorldInfo` | 是否在初始提示词中包含已激活世界书 |
| `tools.allow` / `deny` | 可用工具，deny 优先 |
| `tools.maxRounds` / `maxCallsPerRun` / `maxCallsPerTool` | 每个 Invocation 的轮数与调用预算 |
| `tools.toolDescriptions` | 替换模型看到的工具或参数描述 |
| `skills.visible` / `deny` | 按名称选择可用 Skill |
| `workspace.visibleRoots` / `writableRoots` | 文件读写范围 |
| `run.presentation` | 前台聊天输出或后台文件处理 |
| `run.stream` / `modelRetry` | 流式预览与模型请求重试 |
| `run.directRunnable` | 是否出现在直接运行的 Agent 选择列表 |
| `output.artifacts` | 输出文件；当前正文目标为 `messageBody` |

工具字段使用稳定 ID，例如 `builtin:workspace.read_file` 或 `mcp/<registration-id>:<tool-name>`。模型看到的调用名称由 runtime 生成，见 [工具](ToolSystem.md)。

`plan/` 可以保存普通计划文件。当前运行器支持的 Profile plan 配置是 `mode: "none"`、空 `nodes`，尚无节点式工作流执行器。

## 配置协作

调用方开启 `delegation.canDelegate` 或 `canHandoff`，并允许相应工具。接收方开启 `callable`，再按用途选择 `allowAsSubagent` 或 `allowAsHandoffTarget`；`allowedCallers` 指定可调用它的 Profile，`["*"]` 表示所有调用方。

`descriptionForAgents` 用于向其他 Agent 介绍它适合处理什么工作。并发、任务数量和交接深度也在 `delegation` 中设置。完整流程见 [多 Agent 协作](SubAgent.md)。

## 源码

Profile 以 JSON 保存到 `_tauritavern/agent-profiles/profiles/`，当前 schema 版本为 3。

- [profile.rs](../../src-tauri/crates/tt-domain/src/models/agent/profile.rs)：字段与默认值。
- [agent_profile_service](../../src-tauri/crates/tt-application/src/services/agent_profile_service)：默认配置、解析和验证。
- [profile-model.ts](../../src/scripts/extensions/agent-system/src/profile-model.ts)：Profile 编辑与可移植配置。
- [LLM Connection API](../API/LlmConnections.md)：连接的创建与管理。
