# 工作区

工作区保存 Agent 可以反复处理的文件。同一 Run 中的 Agent 共享工作文件，各自按 Profile 获得读写范围；`skills/` 则按当前 Invocation 的有效 Skill 绑定提供只读文件视图。

## 文件放在哪里

默认 Profile 的工作目录与自动可见只读目录：

| 目录 | 用途 |
| --- | --- |
| `output/` | 准备提交的正文或其他输出，默认正文为 `output/main.md` |
| `scratch/` | 草稿和临时材料 |
| `plan/` | 普通计划文件 |
| `summaries/` | 摘要与子 Agent 结果的可读版本 |
| `persist/` | 本次运行的持久内容工作副本 |
| `tool-results/` | 工具结果及较长结果的可读版本，只读 |
| `skills/` | 当前 Invocation 的有效 Skill 安装包，只读；无绑定时为空目录 |

模型用 `workspace.list_files`、`search_files`、`read_file` 寻找和读取材料，用 `write_file`、`apply_patch` 修改文本，或用 `workspace.shell` 批量处理文件。路径相对于工作区，例如 `output/main.md`；Shell 中的 `/output/main.md` 指向同一文件。

文本工具替换已有文件和应用补丁时，使用读取记录和内容 SHA 检查冲突。Shell 不建立此读取记录；Shell 修改文件后，替换文件或应用补丁前需重新读取。

普通 Run 文件读取返回原文，脚本可用 `macros.render()` 展开模板；Skill 文件的宏规则见 [Skill](Skill.md)。聊天与世界书通过各自工具读取。

## 统一文件链路

文本工具与 Shell（含 Python、JavaScript）通过 `WorkspaceFs` 访问同一逻辑视图。普通文件和 runtime 材料保存在 Run 的真实目录，`WorkspaceRepository` 负责初始化、manifest 与持久版本发布；Skill 原始文件由 `SkillRepository` 读取，应用层负责绑定和宏投影。

模型侧使用当前 Invocation 的 `ScopedWorkspaceFs`；业务根本身不可修改，`tool-results` 和 `skills` 只读。Skill 是外部挂载，不复制到 Run 目录。

Run 工作文件允许并行读取，按单次操作串行修改；CAS 的条件检查与写入在同一锁内完成。多步操作不构成事务，已成功的操作立即生效；Shell 失败或取消不回滚已完成的修改。追加使用原生 append，失败可能部分生效，不自动重放。

## JavaScript

`workspace.shell` 通过 QuickJS 执行 JavaScript ESM，入口为 `js`；`node`、`deno` 是同一受限环境的命令别名，不提供 Node/Deno 标准库。命令和 API 用法见 `js --help`。

脚本入口相对于 Shell 当前目录，import 相对于导入模块；`@tauritavern/runtime` 的文件 API 相对于工作区根。模块按需加载，文件直接读写，遵循上述权限与提交规则。

`context` 和 `macros` 使用 Run 冻结输入，子 Agent 与恢复后的调用继续沿用。缺少聊天上下文不影响普通 JS 和文件操作，访问不可用的 context 字段才报错。

取消停止后续 Shell 调度，等待当前 JS 和已开始的文件操作收尾。收尾以整个 `workspace.shell` 返回为界，内部 `timeout` 不保证单条命令已结束。

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

模型调用 `workspace.commit` 时，runtime 读取指定的可访问 Run 工作文件并请求前端宿主保存。默认操作是替换本次输出楼层的正文；`append` 则将文件内容追加到本次输出。Host bridge 沿用 SillyTavern 的输出处理与保存流程，成功后把结果交回 runtime。

首次显式提交前，前台运行还会展示写作进度：流式写入形成实时正文，符合条件的文本修改会自动提交为进度记录。首次显式提交成功后，后续聊天发布由显式 `workspace.commit` 控制。`workspace.finish` 仍要求前台至少完成一次显式提交。

Shell 与文本工具共用自动提交规则：每轮最多发布最后修改的合格文本文件，提交时读取当前内容。Shell 非零退出、取消或超时会清除本轮待提交候选。

已确认的提交会保留，即使后续运行失败。模型、工具与文件处理的详细过程放在 Timeline；聊天消息保存正文、可见 reasoning 和关联 Run 的 metadata。

## 将内容带到下一次运行

`persist/` 的起点由 `persistBaseStateId` 指定。初始化时，仓储把对应持久版本复制到本次 Run；模型随后像处理普通文件一样修改它。

`workspace.finish` 将 `persist/` 的文件与目录发布为不可变版本，并将其 ID 写入已提交消息的 Agent metadata。版本反映删除、移动和空目录等变化；修订未改变完整状态时复用上一版本。后续生成根据当前消息或 swipe 选择起点，因此不同候选可以保有各自的持久内容。

聊天分叉会复制持久版本并使用新聊天身份。运行历史清理与持久版本清理分别处理：缩减旧 Run 的材料不会删除仍被聊天使用的持久内容。

## 保留与清理

运行历史分为核心记录和完整材料。较近的 Run 保留全部文件，较早的 Run 可以只留 `run.json`、日志和摘要，超过历史窗口的 Run 再整次删除。材料清理后，Timeline 仍能显示保留的事件，对应文件详情可能已不可读。

`api.agent.retention` 提供设置、预览和执行入口；自动清理默认关闭。操作参数见 [Agent API](../API/Agent.md)。

## 源码

- [workspace_policy.rs](../../src-tauri/crates/tt-application/src/services/agent_profile_service/workspace_policy.rs)：目录与 Profile 的对应关系。
- [workspace 工具](../../src-tauri/crates/tt-application/src/services/agent_tools/workspace)：文件读改与提交请求。
- [WorkspaceFs](../../src-tauri/crates/tt-ports/src/workspace_fs.rs)：统一文件契约与文本便利方法。
- [ScopedWorkspaceFs](../../src-tauri/crates/tt-application/src/services/agent_workspace_scope.rs)：Invocation 范围视图。
- [WorkspaceShell adapter](../../src-tauri/crates/tt-adapter-workspace-shell/src)：Shell、JavaScript、执行期文件桥与收尾。
- [FileAgentRepository](../../src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_agent_repository)：路径、文件、持久版本与清理。
- [聊天提交桥](../../src/tauri/main/api/agent-chat-commit-bridge.js)：接入前端聊天保存。
