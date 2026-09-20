# 工具

工具把模型的请求转成一次有结果的操作。模型读取工具描述，发出调用；runtime 检查本次 Invocation 的工具集合，执行操作，再将结果送回下一轮上下文。

## 身份与调用名称

工具目录使用稳定的 `ToolId`。内置工具是 `builtin:<name>`，MCP 工具是 `mcp/<registration-id>:<name>`。Profile 的工具配置和运行记录使用这些 ID。

模型看到的名称由 Invocation 创建时生成。例如 `builtin:workspace.read_file` 对应 `workspace_read_file`。MCP 名称根据服务器和工具名生成。稳定 ID 用于记录和配置，模型名称用于这次调用，两者的对应关系保存在工具快照中。

Invocation 使用的工具快照包含描述、参数、名称映射及预算上限，按工具目录、Profile 和结束方式创建；输出修订沿用原快照。每个 Invocation 的调用计数独立，由 `ToolRequestGate` 检查后进入内置工具或 MCP 的执行路径。

新 Profile 默认启用 Shell。

## 内置工具

下表使用源码中的工具名。参数以 [descriptor 定义](../../src-tauri/crates/tt-application/src/services/agent_tools) 和 `api.agent.tools.list()` 返回的 schema 为准。

| 工作 | 工具 |
| --- | --- |
| 读取聊天 | `chat.search`、`chat.read_messages` |
| 读取激活世界书 | `worldinfo.read_activated` |
| 读取工作文件与 Skill | `workspace.list_files`、`workspace.search_files`、`workspace.read_file` |
| 修改文件 | `workspace.write_file`、`workspace.apply_patch` |
| Shell 与数据处理 | `workspace.shell`，内含 jq、Python 与 JavaScript |
| 发布与结束 | `workspace.commit`、`workspace.finish` |
| 委派与交接 | `agent.delegate`、`agent.await`、`agent.handoff`、`task.return` |
| 掷骰 | `dice.roll` |

聊天工具读取 Run 输入对应的历史范围；文件读取使用 1-based 行号，聊天消息索引使用 0-based。较长文本可以分段读取。完整工具集合会按 Profile 收窄，return-mode 子 Agent 使用 `task.return` 作为结束工具。

可调用 Agent 目录随提示词提供，协作方式与错误反馈见 [多 Agent 协作](SubAgent.md)。

## 参数

一次调用携带一组具名参数。线上要么是 JSON object，要么是它的 JSON 字符串编码，「没有参数」在各家实现里有多种等价写法（字段缺失、`null`、`""`、`"null"`、`"{}"`）。`ToolArguments` 是唯一的换算位置：先剥掉字符串编码，再用同一条规则判断，因此两种编码得到同一个参数集合，全可选参数的工具可以直接无参调用。

既不是 object、也不属于上述空集合写法的内容（截断的 JSON、数组、标量）原样保留。runtime 在预算计入之后、分派之前拒绝这类调用，返回可恢复的 `tool.invalid_arguments` 并回显开头一段原文；工具本身只接收已经确定为 object 的参数。

各渠道回放统一使用对象参数：合法对象保持原值，非法参数回放为 `{}`，拒绝原因由配对的 tool error 承载。持久化仍保留非法参数原文。

## Shell

`workspace.shell` 提供 Bashkit 内置命令、jq、Python 子集与 JavaScript。参数为 `command` 与可选 `workdir`（默认 `/`）。每次调用创建新环境，共享工作区文件保留；不执行宿主外部程序。退出状态与输出沿普通工具结果返回，执行与文件契约见 [Workspace](Workspace.md)。

## 结果如何进入下一轮

`AgentToolResult` 包含调用 ID、工具 ID、文本、结构化结果、错误信息和资源引用。runtime 把它作为 tool message 追加到模型上下文，并保存调用记录。

模型能够修正的请求错误也返回 tool result，例如参数不合法、读取范围不正确或文件已变化。存储和运行状态错误则向 Run 收尾流程传播。聊天中显示的正文通过工作区提交产生。

## MCP 与 Skill

MCP 提供外部工具。MCP Manager 管理服务器、发现目录和调用权限；Agent 使用已发现的工具目录，调用前由 MCP 服务确认权限。较长结果存为 `tool-results/` 下的可读文件，模型收到摘要和文件路径。配置与连接行为见 [MCP](../CurrentState/MCP.md)。

Skill 提供工作方法、材料和脚本，通过只读 `skills/` 视图与 `workspace.shell` 使用，组织与作用域见 [Skill](Skill.md)。

## 添加一个工具

先看相近的内置工具，实现它的参数描述和执行逻辑，再在 `BuiltinAgentToolRegistry` 注册。工具返回内容供模型继续工作；需要文件修改、提交或任务控制时，通过现有 `AgentToolEffect` 交给 runtime 记录和处理。

需要网络、文件或脚本引擎等外部能力时，沿用仓库的 port 与 adapter 边界。验证应从可观察的调用结果和副作用入手，见 [测试](TestingStrategy.md)。

- [registry.rs](../../src-tauri/crates/tt-application/src/services/agent_tools/registry.rs)：内置目录。
- [policy.rs](../../src-tauri/crates/tt-application/src/services/agent_tools/policy.rs)：Invocation 工具快照。
- [tool.rs](../../src-tauri/crates/tt-domain/src/models/tool.rs)：`ToolArguments` 的线上编码换算规则。
- [tool_request_gate.rs](../../src-tauri/crates/tt-application/src/services/tool_request_gate.rs)：调用检查与预算。
- [tool_execution.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/tool_execution.rs)：分派和记录。
- [workspace/shell.rs](../../src-tauri/crates/tt-application/src/services/agent_tools/workspace/shell.rs)：Shell 参数与结果映射。
