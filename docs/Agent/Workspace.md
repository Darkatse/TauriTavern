# 工作区

工作区保存 Agent 可以反复处理的文件。Chat 的工作文件属于 Run，Session 的工作文件由同一会话的历次 Run 共用，各自按 Profile 获得读写范围；`skills/` 按当前 Invocation 的有效 Skill 绑定提供只读文件视图，`chat.json` 与 `floors/` 在 Profile 开了聊天工具时提供当前角色聊天的只读楼层视图。

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
| `chat.json`、`floors/` | 当前角色聊天的概况与楼层，只读，仅 Chat Run 且 Profile 开了聊天工具，见[聊天挂载](#聊天挂载) |

Agent 指令中的 `{{workspace}}` 展开为开局工作区索引，Chat 与 Session 都是如此，见 [Prompt assembly](PromptAssembly.md#开局工作区索引)。模型用 `chat_search`、`chat_read`、`grep`、`read`（及 Profile 开启的 `list`）寻找和读取材料，用 `write`、`edit` 修改文本，或用 `shell` 列目录和批量处理文件；这里写的是模型看到的工具名，与工具 ID 的对应见[工具](ToolSystem.md#身份与调用名称)。路径相对于工作区，例如 `output/main.md`；Shell 中的 `/output/main.md` 指向同一文件。

文本工具替换已有文件和应用补丁时，使用读取记录和内容 SHA 检查冲突。Shell 不建立此读取记录；Shell 修改文件后，替换文件或应用补丁前需重新读取。

普通 Run 文件读取返回原文，脚本可用 `macros.render()` 展开模板；Skill 文件的宏规则见 [Skill](Skill.md)。聊天楼层可经只读挂载读取原文，各读取工具的分工见[工具](ToolSystem.md#身份与调用名称)；单个楼层也可用 `read floors/NNNNNN/message.md` 读取。世界书通过专用工具读取。

## 统一文件链路

文本工具与 Shell（含 Python、JavaScript）通过 `WorkspaceFs` 访问同一逻辑视图。普通文件和 runtime 材料保存在 Run 的真实目录，`WorkspaceRepository` 负责初始化、manifest 与持久版本发布；Skill 原始文件由 `SkillRepository` 读取，应用层负责绑定和宏投影。

模型侧使用当前 Invocation 的 `ScopedWorkspaceFs`；业务根本身不可修改，`tool-results`、`skills`、`chat.json` 和 `floors` 只读。`skills/` 与聊天是外部挂载，不复制到 Run 目录：`ScopedWorkspaceFs` 内部维护一张挂载表，顶层名 `skills`、`chat.json`、`floors` 由挂载提供，其余路径属于 Run 目录。

Run 工作文件允许并行读取，按单次操作串行修改；CAS 的条件检查与写入在同一锁内完成。多步操作不构成事务，已成功的操作立即生效；Shell 失败或取消不回滚已完成的修改。追加使用原生 append，失败可能部分生效，不自动重放。

## 聊天挂载

角色聊天的 Chat Run 把当前聊天挂在工作区根目录，与 `output/`、`persist/` 等并列。一个 Run 只对应一段聊天，路径不带角色或聊天前缀：

```text
chat.json                  title（聊天文件名去扩展名）、stableChatId、character、floorCount
floors/000000/message.md   该楼 `mes` 原文，不展开宏，不经过正则
floors/000000/meta.json    index、name、role、hidden、send_date、swipe_id、swipe_count；有 extra.type 时加 type
```

- 读聊天的权限跟随 Profile：只有允许 `chat.search` 或 `chat.read_messages` 的 Profile 才挂载（`WorkspaceAccessPolicy::from_profile` 调用 `profile_reads_chat`）。没有挂载时这些文件不存在，读取返回 not found，开局索引没有 `Chat` 行，`grep` 也不覆盖楼层。
- 楼号从 0 开始，6 位补零，与 SillyTavern 的 `#mesid` 一致。只包含本次输入冻结前的楼层（`input_message_count` 之前）；隐藏楼层保留。
- `role` 与 `hidden` 由同一个规则（`FloorRole`）得出，`meta.json`、`grep` 与聊天工具都用它：原 `role` 为 `tool` 时是 tool，`is_user` 时是 user，其余是 assistant（旁白也是 assistant，由 `type: narrator` 区分）。`hidden` 表示用户把该楼从提示词里隐藏（`is_system`），不改变 role；Tool 楼层的 `is_system` 只为兼容旧扩展，不算隐藏。`grep` 在路径后标 `[hidden]`，`chat_search`、`chat_read` 显示为「role [hidden]」，structured 带 `hidden: true`。群聊的聊天工具按同一规则显示。`chat_search` 按显示的值过滤：`role` 取 user、assistant 或 tool，隐藏楼层用 `hidden: true` 选出。公开 Chat API 仍以 role `system` 表示隐藏，Agent 工具不使用这个归类；续跑的旧 Run 传 `role: "system"` 时返回可恢复错误，提示改用 `hidden`。
- 某楼的记录缺少字符串 `mes` 时，只有这一楼没有 `message.md`：读取它返回 not found，并写明楼号和原因；列目录照常列出这一楼（只有 `meta.json`）；`grep` 跳过它，并在结果里写明跳过了几楼；`chat_search` 不会命中它，`chat_read` 读它返回 `chat.message_not_found` 并写明原因。其余楼层不受影响。
- 读取、列目录、搜索和 Shell 都能访问；写入、编辑、改名和删除返回只读错误，与 `skills/` 相同。`grep` 搜楼层（不带 `path` 或指向楼层目录）时只匹配 `message.md`，`meta.json` 与 `chat.json` 需直接指定文件；直接扫描快照而非逐文件遍历，结果与 Run 文件一起按路径排列。
- 聊天工具只读聊天快照（`ChatSnapshot`）：角色聊天与群聊都从聊天文件构建同样的楼层（原文、role、hidden、楼层范围和缺 `mes` 的处理）；挂载（`ChatMount`）只是把角色聊天的快照另外呈现为上面的文件。`chat_search` 在楼层原文上按词打分（与聊天搜索 API 共用 `RankedTextSearch`），`chat_read` 按楼号读取，文本、楼层范围与 role 都和楼层文件、`grep` 一致。群聊不挂载，`chat_search` 的命中不带楼层文件路径，改用 `chat_read` 按楼号读取。
- 每个 Invocation 首次访问时（楼层文件、`grep` 或聊天工具）读取整份聊天并在内存中复用，不写入 checkpoint。恢复运行会重新读取；本期不校验恢复前后已有楼层是否一致，期间被编辑的楼层会直接反映在读取结果里。
- Session 与群聊没有此挂载。

## JavaScript

`workspace.shell` 通过 QuickJS 执行 JavaScript ESM，入口为 `js`；`node`、`deno` 是同一受限环境的命令别名，不提供 Node/Deno 标准库。命令和 API 用法见 `js --help`。

脚本入口相对于 Shell 当前目录，import 相对于导入模块；`@tauritavern/runtime` 的文件 API 相对于工作区根。模块按需加载，文件直接读写，遵循上述权限与提交规则。

模块执行顶层代码，业务函数由脚本显式调用，异步工作使用 `await`。参数从 `process.argv` 读取，文件脚本使用 `process.argv.slice(2)` 获取字符串参数；选项和子命令由脚本解析。文件、stdin 与 eval 的参数边界见 `js --help`。

结构化结果显式写入 stdout，诊断写入 stderr；较大的输入输出使用工作区文件。`process.exitCode` 默认是 0，可设置为数字整数 `0–255`，非法赋值抛错。未捕获异常或非零退出码表示失败，遵循既有文件保留与自动提交规则。

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

以下提交和 persist 发布规则只适用于 Chat target。

模型调用 `workspace.commit` 时，runtime 读取指定的可访问 Run 工作文件并请求前端宿主保存。默认操作是替换本次输出楼层的正文；`append` 则将文件内容追加到本次输出。Host bridge 沿用 SillyTavern 的输出处理与保存流程，成功后把结果交回 runtime。

首次显式提交前，前台运行还会展示写作进度：流式写入正文文件时形成实时正文，正文文件的修改会自动提交为进度记录。正文文件是输出策略中 `messageBody` 目标的文件（Default Writer 为 `output/main.md`）；`persist/`、`scratch/` 等其他文件是工作笔记，不会自动进入聊天。首次显式提交成功后，后续聊天发布由显式 `workspace.commit` 控制。`workspace.finish` 仍要求前台至少完成一次显式提交。

Shell 与文本工具共用自动提交规则：每轮最多发布一次正文文件，提交时读取当前内容；同一轮之后写入的其他文件不会取代它。Shell 非零退出、取消或超时会清除本轮待提交候选。

已确认的提交会保留，即使后续运行失败。模型、工具与文件处理的详细过程放在 Timeline；聊天消息保存正文、可见 reasoning 和关联 Run 的 metadata。

## 将内容带到下一次运行

`persist/` 的起点由 `persistBaseStateId` 指定。初始化时，仓储把对应持久版本复制到本次 Run；模型随后像处理普通文件一样修改它。

运行结束时（`workspace.finish` 或带 `finish: true` 的提交），runtime 将 `persist/` 的文件与目录发布为不可变版本，并将其 ID 写入已提交消息的 Agent metadata。版本反映删除、移动和空目录等变化；修订未改变完整状态时复用上一版本。后续生成根据当前消息或 swipe 选择起点，因此不同候选可以保有各自的持久内容。

聊天分叉会复制持久版本并使用新聊天身份。运行历史清理与持久版本清理分别处理：缩减旧 Run 的材料不会删除仍被聊天使用的持久内容。

## 保留与清理

运行历史分为核心记录和完整材料。较近的 Run 保留全部文件，较早的 Run 可以只留 `run.json`、日志和摘要，超过历史窗口的 Run 再整次删除。材料清理后，Timeline 仍能显示保留的事件，对应文件详情可能已不可读。

`api.agent.retention` 提供设置、预览和执行入口；自动清理默认关闭。操作参数见 [Agent API](../API/Agent.md)。

## Session 的持续工作区

```text
agent-workspaces/sessions/
  profile.json                      共享配置
  <session-id>/
    session.json                    会话元数据
    history.jsonl                   连续消息
    workspace/{work,tmp,tool-results}/
    runs/<run-id>/                  每次执行的记录
```

`work/`、`tmp/` 和只读的 `tool-results/` 跨 Run、跨重启保留，临时材料由 Agent 自行清理。Shell 的 `/tmp` 指向工作区目录，不是宿主临时目录。历史与文件按 Session 隔离，只有 Profile 共用。

Session 文件直接修改，不发布 Chat persist 版本。数据仅保存在本机，不参与现有同步或 Chat retention；完整数据归档仍包含它们。

删除 Session 清理其目录与 Run 索引，保留共享 Profile 和 Skill，不回滚工具对应用其他数据的修改。调用约束见 [Session API](../API/Agent.md#持续-session)。

## 源码

- [workspace_policy.rs](../../src-tauri/crates/tt-application/src/services/agent_profile_service/workspace_policy.rs)：目录与 Profile 的对应关系。
- [workspace 工具](../../src-tauri/crates/tt-application/src/services/agent_tools/workspace)：文件读改与提交请求。
- [WorkspaceFs](../../src-tauri/crates/tt-ports/src/workspace_fs.rs)：统一文件契约与文本便利方法。
- [ScopedWorkspaceFs](../../src-tauri/crates/tt-application/src/services/agent_workspace_scope.rs)：Invocation 范围视图。
- [WorkspaceShell adapter](../../src-tauri/crates/tt-adapter-workspace-shell/src)：Shell、JavaScript、执行期文件桥与收尾。
- [FileAgentRepository](../../src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_agent_repository)：路径、文件、持久版本与清理。
- [聊天提交桥](../../src/tauri/main/api/agent-chat-commit-bridge.js)：接入前端聊天保存。
