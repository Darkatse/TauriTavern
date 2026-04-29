# TauriTavern Agent MCP and Skill

本文档定义 MCP 与 Skill 在 Agent 系统中的位置。

结论：

- MCP 是独立平台集成能力，不是 Agent Runtime 本体。
- Skill 是渐进披露的文本/资源包，不是自动吞入 prompt 的大文件。
- Agent 可以消费 MCP 和 Skill，但必须经过 ToolRegistry、ContextFrame、Policy 与 Journal。

当前状态（2026-04-29）：尚未实现 `window.__TAURITAVERN__.api.mcp`，也未把 MCP/Skill 接入 Agent tool registry。当前 Agent registry 已有 Phase 2C 内建工具：`chat.search`、`chat.read_messages`、`worldinfo.read_activated`、`workspace.list_files`、`workspace.read_file`、`workspace.write_file`、`workspace.apply_patch`、`workspace.finish`。

## 1. MCP 边界

MCP 模块建议提供：

```text
McpClientService
  list_servers
  connect_server
  disconnect_server
  list_tools
  call_tool
  list_resources
  read_resource
  list_prompts
  get_prompt
```

Agent 消费方式：

```text
MCP Tools     -> ToolRegistry
MCP Resources -> WorkspaceResource / ContextFrame
MCP Prompts   -> PromptComponent / Preset macro / slash command
```

Agent Runtime 不应建在 MCP 上。MCP server 只是外部能力来源之一。

## 2. MCP Host ABI

非 Agent 模式下也可以提供：

```js
window.__TAURITAVERN__.api.mcp
```

用途：

- 用户在设置或工具面板中查看 server。
- 用户显式调用 tool。
- 扩展读取 MCP resource/prompt。
- Agent Mode 通过同一底层服务消费 MCP。

API 草案见 `docs/API/MCP.md`。

## 3. MCP 安全底线

必须禁止：

- Agent/Preset/角色卡/世界书直接写 MCP stdio command。
- 远端配置自动创建本地 stdio server。
- prompt 修改 MCP config 后自动 reload。
- 初期允许 MCP Sampling 自动发起模型调用。
- 未经审批调用 destructive/high-cost tool。

必须要求：

- server 来自用户设置、系统内置 allowlist 或签名/可信来源。
- command/args 对用户可见。
- per-server capability allowlist。
- per-tool approval policy。
- 所有 MCP tool call 写 journal。

## 4. MCP Tool Result

MCP tool result 应映射为 Agent `ToolResult`：

```text
Text content        -> ToolContentBlock::Text
Image/audio/file    -> ResourceRef / FileRef
Structured content  -> structured JSON
Resource link       -> WorkspaceResource ref
Error               -> is_error = true
```

大结果不能直接塞进 journal；应写 resource ref。

## 5. MCP Resources

MCP resources 默认是 application-driven：

- 宿主决定是否读取。
- ContextAssembly 决定是否纳入 prompt。
- Profile/Preset 决定预算与可见性。

Agent 不应因为 server 暴露 resource 就自动读取全文。

## 6. MCP Prompts

MCP prompts 可以成为：

- PromptComponent。
- Preset macro 的来源。
- 用户显式插入的片段。

它们不应自动覆盖 TauriTavern preset，也不应绕过创作者 policy。

## 7. Skill 定义

Skill 是本地或扩展提供的渐进式知识包。

建议结构：

```text
skills/
  long-form-romance/
    SKILL.md
    examples/
    assets/
```

`SKILL.md` frontmatter 示例：

```yaml
---
name: long-form-romance
description: 长篇恋爱剧情写作技巧
allowed-tools:
  - workspace.read_file
  - workspace.apply_patch
when-to-use: 当需要扩写情感铺垫和细腻心理描写时
context: lazy
---
```

## 8. Skill 读取策略

Agent 默认只看到 skill 索引：

```text
name
description
when-to-use
allowed-tools
```

读取全文必须通过：

```text
skill.read(name, section?, budget?)
```

`skill.read` 是 tool call，必须写 journal。Preset 可以声明某些 skill 自动进入 context，但仍受 budget 管控。

## 9. Skill 来源

Skill 可以来自：

- 用户本地目录。
- Preset 包。
- 角色卡附带资源。
- 扩展。
- 未来 marketplace。

来源不同，信任级别不同。扩展/远端 skill 默认不应获得更高工具权限。

## 10. Skill 与 Workspace

Skill 可以表现为 workspace resource：

```text
skills/<name>/SKILL.md
skills/<name>/examples/foo.md
```

但它默认是 read-only virtual resource。Agent 不能修改原始 skill；如果需要摘录或改写，应写入 `scratch/` 或 `summaries/`。

## 11. Agent Context

Skill 进入 prompt 的路径：

```text
skill.list / preset auto include
  -> SkillService
  -> WorkspaceResource or PromptComponent::Skill
  -> ContextAssemblyService
  -> ModelRequest
```

不要把所有 Skill 全文塞进 system prompt。

## 12. 后续最小实现

Skill 最小实现：

- `SkillRepository` 列出本地 skill。
- `skill.list`。
- `skill.read`。
- profile/preset 控制可见 skill。
- skill content 受 budget 限制。
- tool result 写 journal。

MCP 最小实现应晚于内置工具系统稳定之后。
