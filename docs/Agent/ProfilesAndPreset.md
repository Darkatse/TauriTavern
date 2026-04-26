# Agent Profiles, Preset Schema, and Plan Policy

本文档定义 Agent Profile、Preset Agent 扩展字段、ContextFrame、Prompt Component 与 Plan Policy。

SillyTavern 的核心优势之一是 prompt/preset 的创作者自由。Agent Mode 必须继承这一点，但自由度要进入可维护的 runtime policy，而不是散落在字符串 prompt 中。

## 1. Agent Profile

Agent Profile 是：

```text
Preset + Model/API + Context Policy + Tool Policy + Plan Policy + Output Policy
```

不是单纯的模型选择。

建议领域模型：

```rust
AgentProfile {
    id,
    display_name,
    preset_ref,
    model_ref,
    prompt_policy,
    visible_resource_policy,
    tool_policy,
    plan_policy,
    summary_policy,
    switch_policy,
    output_policy,
    budget_policy,
}
```

### 1.1 Profile 来源

Profile 可以来自：

- 用户手动创建。
- Preset 内嵌 agent schema。
- 角色卡推荐配置。
- 扩展提供的 profile template。

最终运行时必须 resolve 为一个完整、可检查的 `ResolvedAgentProfile`。

### 1.2 Profile Resolution

解析顺序建议：

```text
Built-in defaults
  < Preset agent schema
  < Character agent schema
  < User profile override
  < Per-run override
```

冲突规则必须明确：

- deny 优先于 allow。
- plan node policy 优先于 profile global policy。
- user explicit deny 优先级最高。
- missing required field fail-fast。

## 2. Preset Agent Schema

第一版可以使用 JSON-compatible schema，不必立刻引入 YAML。

示例：

```json
{
  "agent": {
    "enabled": true,
    "profiles": [
      {
        "id": "writer",
        "displayName": "Writer",
        "model": { "source": "openai", "model": "..." },
        "context": {
          "historyBudgetTokens": 12000,
          "workspaceFileBudgetTokens": 6000,
          "toolResultBudgetTokens": 2000,
          "include": [
            "chat.history",
            "world.activated",
            "character.instructions",
            "workspace.output",
            "agent.plan",
            "agent.tool_results"
          ],
          "exclude": ["workspace.scratch.private"]
        },
        "tools": {
          "allow": [
            "workspace.read_file",
            "workspace.apply_patch",
            "chat.search",
            "skill.read"
          ],
          "deny": ["shell.*"],
          "requireApproval": ["mcp.*", "workspace.commit"]
        },
        "output": {
          "artifacts": [
            { "id": "main", "path": "output/main.md", "kind": "body", "target": "message_body" },
            { "id": "status", "path": "output/status.md", "kind": "status", "target": { "message_extra": "status_bar" } }
          ]
        }
      }
    ],
    "defaultProfile": "writer"
  }
}
```

第一期可以只支持：

- `enabled`
- `defaultProfile`
- `profiles[].id`
- `profiles[].model`
- `profiles[].context`
- `profiles[].tools`
- `profiles[].output.artifacts`

更复杂的 plan/profile switch 可以 Phase 3 加。

## 3. ContextFrame

ContextFrame 是 Agent Mode 的 prompt 组织真相。

它应该表达 typed components：

```text
SystemInstruction
ChatHistory
WorldInfo
CharacterCard
UserProfile
PresetGuide
WorkspaceTree
WorkspaceFile
ToolDefinitions
ToolResults
Plan
DiffSummary
Skill
```

每个 component 至少包含：

```text
id
kind
source
visibility
tokenBudget
priority
contentRef or inlineContent
metadata
```

ContextFrame 不是 provider payload。Provider adapter 只消费编译后的 `ModelRequest`。

## 4. Prompt 宏

创作者看到的是宏：

```text
{{agent.plan}}
{{agent.workspace.tree}}
{{agent.file "output/main.md"}}
{{agent.file "scratch/notes.md" budget=800}}
{{agent.tools.available}}
{{agent.tool_results mode="summary" budget=1200}}
{{agent.diff.latest}}
{{agent.skill "long-form-romance"}}
```

宏展开必须生成 typed component 或 component reference，而不是简单字符串替换。

原因：

- provider adapter 可以决定 system/user/tool/resource 位置。
- component 可以独立预算和摘要。
- hidden/private resource 可以被 policy 拒绝。
- prompt cache 可以按 component 做。
- tool result 可以与 chat history 平级。

## 5. Context Budget

Budget 必须可组合：

```text
totalContextBudget
historyBudgetTokens
workspaceFileBudgetTokens
toolResultBudgetTokens
skillBudgetTokens
worldInfoBudgetTokens
summaryBudgetTokens
```

超预算策略：

```text
truncate
summarize
drop_optional
fail
```

默认建议：

- required component 超预算：fail-fast。
- optional component 超预算：按 priority drop，并写 `context_component_skipped` event。
- tool result 超预算：优先摘要。
- chat history 超预算：使用 windowed read/search + summary。

## 6. Tool Policy

Tool policy 应能表达：

```json
{
  "allow": ["workspace.*", "chat.search"],
  "deny": ["shell.*"],
  "requireApproval": ["mcp.*"],
  "maxCallsPerRun": 20,
  "maxCallsPerTool": {
    "chat.search": 5
  }
}
```

解析规则：

1. user deny 最高。
2. plan node deny/allow 覆盖 profile global allow。
3. deny 优先 allow。
4. requireApproval 不等于 deny。
5. 未显式 allow 的工具默认不可见，除非 profile 选择 permissive mode。

建议默认 conservative mode：不在 allow list 的工具不可见。

## 7. Visible Resource Policy

资源可见性要与工具独立。

示例：

```json
{
  "include": [
    "chat.history.tail",
    "world.activated",
    "workspace.output",
    "agent.plan"
  ],
  "exclude": [
    "workspace.scratch.private",
    "user.secrets",
    "mcp.resource.private"
  ]
}
```

Agent 不能通过 `workspace.read_file` 绕过 hidden resource policy。

## 8. Plan Policy

Plan Mode 有三种：

```text
free
strict
hybrid
```

### 8.1 Free Plan

Agent 可以创建和修改计划。

运行时仍必须要求：

- 先产出 plan。
- 每个阶段结束 checkpoint。
- 完成前检查 artifact manifest。
- 不能突破全局 tool/resource/budget policy。

### 8.2 Strict Plan

Preset/创作者提供固定节点。

Agent 不能：

- 改节点顺序。
- 跳过 locked 节点。
- 使用节点外工具。
- 切换到节点外 profile。
- 写节点外 expected artifact，除非 output policy 允许。

违反必须 fail-fast 或进入 approval，不能静默继续。

### 8.3 Hybrid Plan

部分 locked，部分 free。

推荐作为高级默认模式：

```text
outline locked
write free
polish locked
```

它同时保留创作者控制和模型发挥空间。

## 9. Plan Node

建议模型：

```rust
PlanNode {
    id,
    title,
    locked,
    profile_id,
    allowed_tools,
    visible_files,
    max_rounds,
    context_budget,
    expected_artifacts,
    approval_required,
}
```

Plan node 开始/完成必须写 journal：

```text
plan_node_started
plan_node_completed
```

## 10. Profile Switch

Profile switch 可以来自：

- plan node 指定。
- model request。
- runtime policy。
- user override。

必须检查：

- current plan node 是否允许 switch。
- target profile 是否存在。
- target profile 的 tool/resource/model policy 是否满足平台限制。
- switch 次数是否超 budget。

结果必须写 journal：

```text
profile_switch_requested
profile_switched
profile_switch_denied
```

## 11. 创作者自由与安全边界

创作者可以控制：

- Agent 可见哪些内容。
- Agent 可用哪些工具。
- 输出有哪些 artifact。
- 哪些阶段严格，哪些阶段自由。
- 是否需要审批。
- token/tool/context budget。
- profile/model 切换策略。

创作者不能控制：

- workspace root 之外的文件访问。
- MCP stdio command。
- 平台 policy 禁用的 provider/source/endpoint override。
- 用户显式 deny 的工具。
- journal 是否记录副作用。
- commit 是否绕过保存契约。

## 12. MVP Profile

当前状态（2026-04-26）：Phase 2A 尚未实现 profile resolution / profile routing。`profileId` 可以随 run 记录，但不会驱动模型、工具或 context policy。当前工具 registry 固定为 `workspace.write_file` 与 `workspace.finish`，输出 artifact 固定为 `output/main.md`。

Phase 1/2A 最小 profile 可以理解为硬编码：

```json
{
  "id": "default-writer",
  "modelRef": { "source": "openai", "model": "current" },
  "contextPolicy": {
    "mode": "prompt_snapshot"
  },
  "toolPolicy": {
    "allow": ["workspace.write_file", "workspace.finish"],
    "deny": ["*"]
  },
  "outputPolicy": {
    "artifacts": [
      { "id": "main", "path": "output/main.md", "target": "message_body", "required": true }
    ]
  }
}
```

这个 profile 足以支撑 Phase 2A 最小工具循环，同时给 Phase 2B/3 留出自然扩展点。
