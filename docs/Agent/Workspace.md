# TauriTavern Agent Workspace

本文档定义 Agent Workspace 的存储模型、路径约束、Artifact Assembly、Checkpoint 与回滚语义。

Workspace 是 Agent Mode 的中心抽象。Agent 不直接写聊天消息，而是在 workspace 中像编辑项目文件一样多轮修改输出，最后由 runtime 提交 artifact。

## 1. 核心目标

Workspace 解决四个问题：

1. 多轮编辑：允许模型反复修改草稿、计划、状态栏、小剧场等文件。
2. 可审计：每次写入、patch、checkpoint 都能追踪。
3. 可回滚：用户不满意时回到某个 checkpoint，而不是只能重新生成。
4. 可组合：最终聊天消息可以由多个 artifact 组装，而不是只有单个 output。

## 2. 两级 Workspace

推荐使用两级结构：

```text
agent-workspaces/
  chats/
    <chat-workspace-id>/
      resources/
        world/
        character/
        preset/
        user/
        skills/
        memory/
      runs/
        <run-id>/
          manifest.json
          input/
          output/
          plan/
          scratch/
          summaries/
          checkpoints/
          patches/
          events.jsonl
```

实际物理根目录为数据根下的 `_tauritavern/agent-workspaces`。它是 TauriTavern 运行产物空间，不属于上游 SillyTavern `default-user` 聊天文件布局。

`<chat-workspace-id>` 必须由稳定聊天身份派生：

```text
stableChatId = window.__TAURITAVERN__.api.chat.open(chatRef).stableId()
chatWorkspaceId = "chat_" + sha256({ kind, stableChatId })[0..16]
```

它不得直接使用可变的 chat file name、角色显示名或完整 `chatRef` hash。聊天重命名、角色卡显示名变化、前端当前引用变化，不应该让同一个稳定聊天分裂到新的 chat workspace。

### 2.1 Chat Workspace

对话级 workspace 保存长期资源引用或 materialized 快照：

- 当前角色卡的 Agent 指导文件。
- 当前 preset 的 Agent policy。
- 用户侧写。
- 可用 skill 索引。
- 长期 memory/resource 索引。

它不应该被某一次 regenerate/swipe 污染。

同一稳定聊天的 normal、regenerate、swipe、continue 等多次 Agent run 共享同一个 chat workspace，但每次 run 必须有独立 run workspace 与独立 journal。

### 2.2 Run Workspace

一次 Agent Run 拥有独立 run workspace：

- 本次输入快照。
- 本次计划。
- 本次 scratch。
- 本次输出 artifact。
- 本次 checkpoint。
- 本次 journal。

这样不同 run 可以比较、回滚、删除或保留，不互相覆盖。

## 3. Resource 类型

“万物皆文件”是 Agent 视角的抽象，不代表所有数据都必须物理复制。

Workspace resource 分三类：

```text
MaterializedFile
  已落盘到 run workspace 的文本或二进制文件。

VirtualResource
  看起来像文件，但内容由 repository/tool 按需读取。

GeneratedArtifact
  Agent 生成或修改、准备参与提交的输出文件。
```

示例：

```text
input/prompt_snapshot.json          MaterializedFile
input/world/activated.md            MaterializedFile
input/preset/instructions.md        MaterializedFile
input/character/card.md             MaterializedFile
chat/history.tail.md                VirtualResource
chat/history.search://query=...     VirtualResource
skills/long-form-romance/SKILL.md   VirtualResource or MaterializedFile
output/main.md                      GeneratedArtifact
output/status.md                    GeneratedArtifact
```

原则：

- 大历史、大世界书、大记忆库默认 virtual。
- 本次生成必须固定的输入可以 materialize，但 “materialize input/context” 不等于复制完整上下文。
- 模型要改的文件必须是 materialized/generated，不能直接修改 virtual resource。

## 4. 路径契约

WorkspacePath 必须是逻辑路径，不直接等同系统路径。

必须满足：

- 相对路径。
- 使用 `/` 作为分隔符。
- 非空。
- UTF-8。
- normalize 后不能包含 `..`。
- 不能是绝对路径。
- 不能包含 NUL。
- 不能包含 Windows drive prefix。
- 不能逃出 workspace root。

禁止：

```text
../secrets.json
/Users/me/file
C:\Users\me\file
output/../../chat.jsonl
scratch/\0bad
```

所有 workspace file operation 必须经由 WorkspaceService 统一校验。工具、MCP、extension bridge 不得自行拼接文件系统路径。

## 5. Manifest

每个 run workspace 必须有 `manifest.json`。

建议结构：

```json
{
  "workspaceVersion": 1,
  "runId": "run_...",
  "stableChatId": "stable-chat-id",
  "chatRef": {
    "kind": "character",
    "characterId": "...",
    "fileName": "..."
  },
  "createdAt": "2026-04-26T00:00:00Z",
  "input": {
    "mode": "prompt_snapshot",
    "promptSnapshotPath": "input/prompt_snapshot.json"
  },
  "artifacts": [
    {
      "id": "main",
      "path": "output/main.md",
      "kind": "body",
      "target": "message_body",
      "required": true,
      "assemblyOrder": 10
    },
    {
      "id": "status",
      "path": "output/status.md",
      "kind": "status",
      "target": { "message_extra": "status_bar" },
      "required": false,
      "assemblyOrder": 20
    },
    {
      "id": "theater",
      "path": "output/theater.md",
      "kind": "side_scene",
      "target": "combined_markdown",
      "required": false,
      "assemblyOrder": 30
    }
  ],
  "commitPolicy": {
    "defaultTarget": "message_body",
    "combineTemplate": "{{main}}\n\n---\n\n{{theater}}",
    "storeArtifactsInExtra": true
  }
}
```

Manifest 是 runtime contract：

- required artifact 缺失必须 fail-fast。
- unknown target 必须 fail-fast。
- artifact path 违反 WorkspacePath 必须 fail-fast。
- commitPolicy 不合法必须 fail-fast。

## 6. 目录职责

```text
input/
  本次 run 的不可变输入快照。

output/
  准备提交给聊天消息或 extra 的 artifact。

plan/
  runtime 可检查的计划文件和用户/模型可读计划。

scratch/
  Agent 私有草稿。默认不提交，是否进入 context 由 policy 决定。

summaries/
  对历史、工具结果、前序步骤的摘要。

checkpoints/
  checkpoint snapshot 与 manifest。

patches/
  可选 patch 记录。第一期可以只做 snapshot。

events.jsonl
  append-only run journal。
```

## 7. Artifact Assembly

Artifact Assembly 把多个 workspace 文件组装为 chat message。

Artifact target：

```text
MessageBody
  写入 chat message `mes`。

MessageExtra(key)
  写入 chat message extra 的 TauriTavern namespace。

CombinedMarkdown(template)
  按模板合并到 message body。

HiddenRunArtifact
  不进入 chat，仅保留在 run workspace。
```

建议优先级：

1. 第一版只要求 `output/main.md` -> `mes`。
2. 同时保留 manifest 能力，允许后续扩展状态栏、小剧场、变量。
3. optional artifact 缺失时跳过。
4. required artifact 缺失时 fail-fast。

## 8. Commit 语义

Commit 是 workspace 到 chat 的边界。

必须：

- 在 commit 前创建 checkpoint。
- 读取 manifest。
- assemble artifacts。
- 通过现有 chat 保存契约写入。
- 写入 agent metadata。
- journal 记录 `artifact_assembled` 与 `run_committed`。

禁止：

- 直接写 JSONL。
- 绕过 windowed payload cursor 保护。
- 由工具自行 commit。
- commit 半成品 artifact。

Commit metadata 建议：

```json
{
  "tauritavern": {
    "agent": {
      "runId": "run_...",
      "stableChatId": "stable-chat-id",
      "checkpointId": "ckpt_...",
      "profileId": "writer",
      "artifactSetId": "artifact_set_...",
      "artifacts": [
        { "id": "main", "kind": "body", "path": "output/main.md" }
      ]
    }
  }
}
```

## 9. Checkpoint

Checkpoint 是 run 内回滚和 commit 后追踪的基础。

第一期建议 snapshot，不引入 git。

结构：

```text
checkpoints/
  000001/
    checkpoint.json
    manifest.json
    output/
      main.md
      status.md
    plan/
      plan.md
    summaries/
      ...
```

`checkpoint.json`：

```json
{
  "id": "ckpt_...",
  "seq": 1,
  "runId": "run_...",
  "createdAt": "2026-04-26T00:00:00Z",
  "reason": "after_workspace_patch",
  "eventSeq": 42,
  "files": [
    { "path": "output/main.md", "sha256": "...", "bytes": 1024 }
  ]
}
```

Checkpoint 时机：

- workspace 初始化后。
- plan 创建后。
- 每次 workspace-mutating tool 后。
- 每个 plan node 完成后。
- artifact assembly 前。
- commit 前。

## 10. 回滚

### 10.1 Run 内回滚

Run 内回滚只恢复 workspace：

```text
rollback(runId, checkpointId)
  -> restore workspace files
  -> append rollback event
  -> status remains Running/AwaitingApproval or becomes Paused
```

它不修改 chat。

### 10.2 Commit 后回滚

Commit 后回滚修改 chat message：

```text
rollbackCommittedMessage(runId, checkpointId)
  -> assemble artifacts from checkpoint
  -> replace/delete committed chat message
  -> save through chat save contract
  -> append rollback committed event
```

必须遵守 windowed payload 保存串行化。

## 11. Retention

默认 retention 应保守：

- Completed run 可以保留完整 workspace。
- Failed/Cancelled run 默认保留，便于 debug。
- 移动端可以限制 checkpoint 数量或总大小，但删除必须明确记录。
- 用户删除聊天时，关联 workspace 的清理策略需要独立设计，不能静默泄漏大量文件。

建议后续提供设置：

```text
keepCompletedRuns
keepFailedRuns
maxRunWorkspacesPerChat
maxCheckpointBytesPerRun
autoPruneAfterDays
```

## 12. 性能约束

- 不复制完整聊天历史。
- 不把大 virtual resource materialize 到每个 run。
- checkpoint 第一版只 snapshot 小文本文件；大文件使用引用或跳过，并在 manifest 声明。
- workspace tree 展示应懒加载。
- timeline 读取 journal 应支持分页。

## 13. MVP Workspace

Phase 2A 当前最小文件：

```text
_tauritavern/agent-workspaces/
  index/
    runs/
      <run-id>.json
  chats/
    <workspace-id>/
      runs/
        <run-id>/
          manifest.json
          events.jsonl
          input/
            prompt_snapshot.json
          output/
            main.md
          tool-results/
            <tool-call-id>.json
          checkpoints/
            <checkpoint-id>.json
```

Phase 2A 模型可写路径前缀：

```text
output/
scratch/
plan/
summaries/
```

Phase 1 早期设计中的最小概念结构仍可作为抽象理解：

```text
runs/<run-id>/
  manifest.json
  input/prompt_snapshot.json
  output/main.md
  checkpoints/000001/checkpoint.json
  checkpoints/000001/output/main.md
  events.jsonl
```

这个结构足以支撑：

- one-step run
- journal
- checkpoint
- artifact commit
- rollback 基础数据结构（Phase 2A 尚未开放 rollback API）
- 后续工具循环增量接入
