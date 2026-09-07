# 工作区

工作区保存 Agent 可以反复处理的文件。一次 Run 拥有一份工作区；同一 Run 中的 Agent 共享文件，各自按 Profile 获得读写范围。

## 文件放在哪里

默认 Profile 使用下列目录：

| 目录 | 用途 |
| --- | --- |
| `output/` | 准备提交的正文或其他输出，默认正文为 `output/main.md` |
| `scratch/` | 草稿和临时材料 |
| `plan/` | 普通计划文件 |
| `summaries/` | 摘要与子 Agent 结果的可读版本 |
| `persist/` | 本次运行的持久内容工作副本 |
| `tool-results/` | 工具结果及较长结果的可读版本，只读 |

模型用 `workspace.list_files`、`search_files`、`read_file` 寻找和读取材料，用 `write_file`、`apply_patch` 修改文件。路径相对于工作区，例如 `output/main.md`。替换已有文件和应用补丁时，工具使用读取记录和内容 SHA 检查冲突；发生冲突后重新读取，再决定如何修改。

工作区读取返回文件原文。脚本需要展开模板时，可以显式调用 `macros.render()`。聊天、世界书和 Skill 的读取由各自工具完成，保留原有数据来源。

## Run 与聊天的关系

磁盘数据位于数据目录的 `_tauritavern/agent-workspaces/`：

```text
agent-workspaces/
  index/
    runs/<run-id>.json
  chats/<workspace-id>/
    persistent-states/<state-id>/
      manifest.json
      persist/...
    runs/<run-id>/
      run.json
      manifest.json
      events.jsonl
      input/...
      invocations/...
      tasks/...
      agent-results/...
      model-responses/...
      tool-args/...
      tool-results/...
      output/...
      scratch/...
      plan/...
      summaries/...
      persist/...
```

`workspaceId` 由聊天种类和 `stableChatId` 派生。聊天文件名用于定位当前聊天，稳定身份用于关联历次运行。每次生成都创建新的 `runId`。

`manifest.json` 描述工作区目录与输出；`input/` 保存提示词、Profile 和持久内容的起点。Invocation、任务与模型响应等目录供 runtime 和详情 API 使用。

## 提交到聊天

模型调用 `workspace.commit` 时，runtime 读取指定文件并请求前端宿主保存。默认操作是替换本次输出楼层的正文；`append` 则将文件内容追加到本次输出。Host bridge 沿用 SillyTavern 的输出处理与保存流程，成功后把结果交回 runtime。

首次显式提交前，前台运行还会展示写作进度：流式写入形成实时正文，符合条件的文本修改会自动提交为进度记录。首次显式提交成功后，后续聊天发布由显式 `workspace.commit` 控制。`workspace.finish` 仍要求前台至少完成一次显式提交。

已确认的提交会保留，即使后续运行失败。模型、工具与文件处理的详细过程放在 Timeline；聊天消息保存正文、可见 reasoning 和关联 Run 的 metadata。

## 将内容带到下一次运行

`persist/` 的起点由 `persistBaseStateId` 指定。初始化时，仓储把对应持久版本复制到本次 Run；模型随后像处理普通文件一样修改它。

`workspace.finish` 收尾时发布一个新的 `persistent-states/<state-id>/`，并将其 ID 写入已提交消息的 Agent metadata。后续生成根据当前消息或 swipe 选择起点，因此不同候选可以保有各自的持久内容。前端负责选择版本，仓储负责保存版本。当前持久内容支持新增与修改。

聊天分叉会复制持久版本并使用新聊天身份。运行历史清理与持久版本清理分别处理：缩减旧 Run 的材料不会删除仍被聊天使用的持久内容。

## 保留与清理

运行历史分为核心记录和完整材料。较近的 Run 保留全部文件，较早的 Run 可以只留 `run.json`、日志和摘要，超过历史窗口的 Run 再整次删除。材料清理后，Timeline 仍能显示保留的事件，对应文件详情可能已不可读。

`api.agent.retention` 提供设置、预览和执行入口；自动清理默认关闭。操作参数见 [Agent API](../API/Agent.md)。

## 源码

- [workspace_policy.rs](../../src-tauri/crates/tt-application/src/services/agent_profile_service/workspace_policy.rs)：目录与 Profile 的对应关系。
- [workspace 工具](../../src-tauri/crates/tt-application/src/services/agent_tools/workspace)：文件读改与提交请求。
- [FileAgentRepository](../../src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_agent_repository)：路径、文件、持久版本与清理。
- [聊天提交桥](../../src/tauri/main/api/agent-chat-commit-bridge.js)：接入前端聊天保存。
