# 工具

工具把模型的请求转成一次有结果的操作。模型读取工具描述，发出调用；runtime 检查本次 Invocation 的工具集合，执行操作，再将结果送回下一轮上下文。

## 身份与调用名称

工具目录使用稳定的 `ToolId`。内置工具是 `builtin:<name>`，MCP 工具是 `mcp/<registration-id>:<name>`，扩展工具是 `extension/<extensionId>:<name>`。Profile 的工具配置和运行记录使用这些 ID。

模型调用名称在 Invocation 创建时生成，与稳定 ID 的映射保存在工具快照中，续跑沿用快照中的名称。内置工具中，工作区工具使用模型熟悉的短名称，`chat.read_messages` 与 `chat_search` 并列写作 `chat_read`，其余保留命名空间：

| 工具 ID（`builtin:` 之后） | 模型名 |
| --- | --- |
| `workspace.read_file` | `read` |
| `workspace.write_file` | `write` |
| `workspace.apply_patch` | `edit` |
| `workspace.search_files` | `grep` |
| `workspace.list_files` | `list` |
| `workspace.shell` | `shell` |
| `workspace.commit` | `commit` |
| `chat.read_messages` | `chat_read` |
| 其余内置工具 | 把 `.` 换成 `_`，如 `chat_search`、`worldinfo_read_activated`、`agent_handoff`、`task_return` |

扩展使用注册的 `name`，MCP 使用服务器与工具名，按模型协议规范字符和长度，重名时加 `__2` 等短后缀。

模型调用本轮未提供的名称时，runtime 返回可恢复错误 `model.unknown_tool_call`，列出本轮可用的名称；使用旧名称（如 `workspace_write_file`）时提示对应的新名称。这类调用计入 Invocation 调用预算。

工具准备按目录、Chat/Session 场景和 Profile 选择生成 bindings，Invocation 再单独应用完成协议，形成包含描述、参数、名称映射及预算的快照。输出修订沿用原快照。每个 Invocation 的调用计数独立，由 `ToolRequestGate` 检查后明确分派到 Builtin、MCP 或 Extension。

新 Profile 默认启用 Shell。`Default Writer` 提供 `chat.search`、`chat.read_messages`、`workspace.search_files`、`workspace.read_file`、`workspace.write_file`、`workspace.apply_patch`、`workspace.shell` 与 `workspace.commit`：提示词已包含激活世界书和预算内的完整历史，更早的楼层是[聊天挂载](Workspace.md#聊天挂载)中的文件。以下写模型看到的名字：按问题找用 `chat_search`，一次读多楼用 `chat_read`，找精确的词或正则用 `grep`。运行以 `commit(finish: true)` 结束，或在提交后只回文字，由[结束策略](Runtime.md#结束与交接)（`FinishPolicy`）决定。其余内置工具可在复制的 Profile 中开启；已保存的 Profile 保持原有工具，其中已下线的工具在加载时去掉。

## 内置工具

下表使用源码中的工具名。参数以 [descriptor 定义](../../src-tauri/crates/tt-application/src/services/agent_tools) 和 `api.agent.tools.list()` 返回的 schema 为准。

| 工作 | 工具 |
| --- | --- |
| 读取聊天 | `chat.search`、`chat.read_messages` |
| 读取激活世界书 | `worldinfo.read_activated` |
| 读取工作文件与 Skill | `workspace.list_files`、`workspace.search_files`、`workspace.read_file` |
| 修改文件 | `workspace.write_file`、`workspace.apply_patch` |
| Shell 与数据处理 | `workspace.shell`，内含 jq、Python 与 JavaScript |
| 发布与结束 | `workspace.commit`（`finish: true` 提交后结束）；只回文字的一轮按[结束策略](Runtime.md#结束与交接)处理 |
| 委派与交接 | `agent.delegate`、`agent.await`、`agent.handoff`、`task.return` |
| 掷骰 | `dice.roll` |

文件工具使用通用参数名：`file_path`，读取范围为 `offset`（1-based 起始行）与 `limit`；`workspace.search_files` 与 `workspace.list_files` 接受文件或目录，使用 `path`。`workspace.search_files`（模型名 `grep`）只有 `pattern` 与 `path` 两个参数，按行做正则匹配（Rust regex 语法，`(?i)` 忽略大小写）。不带 `path` 时搜索 `tool-results/`、`skills/` 以外的可见目录和聊天楼层；带 `path` 时只搜该文件或子树，`tool-results/`、`skills/` 也可这样搜到。结果按路径和行号排列，长行只保留首个匹配附近约 300 个字符，最多列出 100 行并给出总数，非法正则返回可恢复的 `workspace.grep_pattern_invalid`。`workspace.commit` 须附一句 `reason`，说明本次提交的内容与原因，供模型自查并记入运行记录。

聊天工具把聊天消息称为楼层（floor），参数为 `start_floor`、`end_floor` 与 `floors[].floor`，按楼读取的行范围与 `read` 一样用 `offset`、`limit`（`worldinfo.read_activated` 的 `entries[]` 也是）。角色聊天中 `chat.search` 的每条命中给出楼层文件路径，结果与 `resourceRefs` 同时带上该路径。楼号、`role` 与 `hidden`、按原文读取以及 `grep` 怎样匹配楼层，见[聊天挂载](Workspace.md#聊天挂载)。较长文本可以分段读取。完整工具集合会按 Profile 收窄，return-mode 子 Agent 使用 `task.return` 作为结束工具。

内置工具的参数表是封闭的，数组元素里的对象也一样：runtime 按当前定义拒绝未知参数（包括按旧 schema 续跑的调用），改名的参数会提示新名称；数组元素里的键在改名表中写作 `floors[].index`，提示给出调用中的位置，如 `floors[1].index` 提示改用 `floors[1].floor`。已保存 Profile 中写给旧参数名（`path`、`start_line`、`line_count`，以及聊天工具的 `start_message`、`end_message`、`messages`）的描述覆盖会在加载时迁移到新名称。`workspace.search_files` 已从按词打分搜索改为正则，迁移把这个工具标为语义已变：覆盖里只要写了它的旧参数（`query`、`limit`、`context_lines`）之一，就说明是给旧版写的，这些参数说明连同该工具的工具级描述一起去掉，不搬到 `pattern`，清空的覆盖整条删除。已知限制：只改了工具级描述、没写旧参数的覆盖无法与新写的覆盖区分，会原样保留。已下线的内置工具（如 `workspace.finish`）在加载时从工具配置中去掉，续跑的旧 Run 调用它们时按未知工具返回可恢复错误。

可调用 Agent 目录随提示词提供，协作方式与错误反馈见 [多 Agent 协作](SubAgent.md)。

## 参数

一次调用携带一组具名参数。线上要么是 JSON object，要么是它的 JSON 字符串编码，「没有参数」在各家实现里有多种等价写法（字段缺失、`null`、`""`、`"null"`、`"{}"`）。`ToolArguments` 是唯一的换算位置：先剥掉字符串编码，再用同一条规则判断，因此两种编码得到同一个参数集合，全可选参数的工具可以直接无参调用。

既不是 object、也不属于上述空集合写法的内容（截断的 JSON、数组、标量）原样保留。runtime 在预算计入之后、分派之前拒绝这类调用，返回可恢复的 `tool.invalid_arguments` 并回显开头一段原文；工具本身只接收已经确定为 object 的参数。

各渠道回放统一使用对象参数：合法对象保持原值，非法参数回放为 `{}`，拒绝原因由配对的 tool error 承载。持久化仍保留非法参数原文。

## Shell

`workspace.shell` 提供 Bashkit 内置命令、jq、Python 子集与 JavaScript。参数为 `command` 与可选 `workdir`（默认 `/`）。每次调用创建新环境，共享工作区文件保留；不执行宿主外部程序。退出状态与输出沿普通工具结果返回，执行与文件契约见 [Workspace](Workspace.md)。

## 结果如何进入下一轮

`AgentToolResult` 包含调用 ID、工具 ID、文本、结构化结果、错误信息和资源引用。模型读取其中的 `content`，错误结果带有明确的错误标记；结构化元数据保留在记录中，不自动展开为模型文字。

面向模型的文字各司其职：提示词说明职责、可读写目录与完成方式，工具说明用途与参数，结果报告事实，错误说明问题与已知的纠正办法。访问与读写顺序等规则由 runtime 执行并在错误中说明，不在工具描述里重复；运行时文字使用本轮快照中的工具名称。以理解成本衡量简洁，保留必要的内容边界和后续指引；内部配置、身份与审计细节留在记录中。

MCP 与扩展共用结果审计和长结果处理。超过 [Profile 内联阈值](ProfilesAndPreset.md#调整工作方式) 时，原始 JSON 留作审计，完整可读内容写入只读 `tool-results/` 文件；模型收到开头预览、字符上限、不完整说明及文件路径，并按可用工具给出读取、分页或搜索指引。中断历史保留调用与结果的配对；结果未知时，提醒重复操作前检查状态。

模型能够修正的请求错误也返回 tool result，例如参数不合法、读取范围不正确或文件已变化。存储和运行状态错误则向 Run 收尾流程传播。聊天中显示的正文通过工作区提交产生。

## MCP 与 Skill

MCP Manager 管理服务器、发现目录和调用权限；Agent 使用已发现的目录，调用前由 MCP 服务确认权限。配置与连接行为见 [MCP](../CurrentState/MCP.md)。

Skill 提供工作方法、材料和脚本，通过只读 `skills/` 视图与 `workspace.shell` 使用，组织与作用域见 [Skill](Skill.md)。

## 扩展工具

扩展在启动时注册普通 JS 函数，适用于 Chat、Session 或两者，见 [注册扩展工具](../API/Agent.md#注册扩展工具)。Tauri host 负责 WebView 通信与回执等待，通过 `tt-ports::extension_tools::ExtensionTools` 向 runtime 提供目录和调用能力。

执行时按稳定 ID 查找当前启用的函数，工具快照不保存函数实例。扩展返回普通 JSON，MCP 响应按自身协议整理，随后进入共同的结果处理链路。

## 添加一个工具

先看相近的内置工具，实现它的参数描述和执行逻辑，再在 `BuiltinAgentToolRegistry` 注册。工具返回内容供模型继续工作；需要文件修改、提交或任务控制时，通过现有 `AgentToolEffect` 交给 runtime 记录和处理。

需要网络、文件或脚本引擎等外部能力时，沿用仓库的 port 与 adapter 边界。验证应从可观察的调用结果和副作用入手，见 [测试](TestingStrategy.md)。

- [registry.rs](../../src-tauri/crates/tt-application/src/services/agent_tools/registry.rs)：内置目录。
- [policy.rs](../../src-tauri/crates/tt-application/src/services/agent_tools/policy.rs)：Invocation 工具快照。
- [tool.rs](../../src-tauri/crates/tt-domain/src/models/tool.rs)：`ToolArguments` 的线上编码换算规则。
- [tool_request_gate.rs](../../src-tauri/crates/tt-application/src/services/tool_request_gate.rs)：调用检查与预算。
- [tool_execution.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/tool_execution.rs)：分派和记录。
- [tool_catalog.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/tool_catalog.rs)：目录、可用性和 Profile 新启用校验。
- [tool_results.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/tool_results.rs)：结果映射、审计与共同长结果投影。
- [workspace/shell.rs](../../src-tauri/crates/tt-application/src/services/agent_tools/workspace/shell.rs)：Shell 参数与结果映射。
