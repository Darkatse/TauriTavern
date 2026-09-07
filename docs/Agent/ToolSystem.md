# 工具

工具把模型的请求转成一次有结果的操作。模型读取工具描述，发出调用；runtime 检查本次 Invocation 的工具集合，执行操作，再将结果送回下一轮上下文。

## 身份与调用名称

工具目录使用稳定的 `ToolId`。内置工具是 `builtin:<name>`，MCP 工具是 `mcp/<registration-id>:<name>`。Profile 的工具配置和运行记录使用这些 ID。

模型看到的名称由 Invocation 创建时生成。例如 `builtin:workspace.read_file` 对应 `workspace_read_file`。MCP 名称根据服务器和工具名生成。稳定 ID 用于记录和配置，模型名称用于这次调用，两者的对应关系保存在工具快照中。

每个 Invocation 根据工具目录、Profile 和结束方式创建一份工具快照，包含描述、参数、名称映射及调用预算。调用先经过 `ToolRequestGate`，再进入内置工具或 MCP 的执行路径。

## 内置工具

下表使用源码中的工具名。参数以 [descriptor 定义](../../src-tauri/crates/tt-application/src/services/agent_tools) 和 `api.agent.tools.list()` 返回的 schema 为准。

| 工作 | 工具 |
| --- | --- |
| 读取聊天 | `chat.search`、`chat.read_messages` |
| 读取激活世界书 | `worldinfo.read_activated` |
| 查找和使用 Skill | `skill.list`、`skill.search`、`skill.read`、`skill.run_script` |
| 读取文件 | `workspace.list_files`、`workspace.search_files`、`workspace.read_file` |
| 修改文件 | `workspace.write_file`、`workspace.apply_patch` |
| 发布与结束 | `workspace.commit`、`workspace.finish` |
| 委派与交接 | `agent.list`、`agent.delegate`、`agent.await`、`agent.handoff`、`task.return` |
| 掷骰 | `dice.roll` |

聊天工具读取 Run 输入对应的历史范围；文件读取使用 1-based 行号，聊天消息索引使用 0-based。较长文本可以分段读取。完整工具集合会按 Profile 收窄，return-mode 子 Agent 使用 `task.return` 作为结束工具。

## 结果如何进入下一轮

`AgentToolResult` 包含调用 ID、工具 ID、文本、结构化结果、错误信息和资源引用。runtime 把它作为 tool message 追加到模型上下文，并保存调用记录。

模型能够修正的请求错误也返回 tool result，例如参数不合法、读取范围不正确或文件已变化。存储和运行状态错误则向 Run 收尾流程传播。聊天中显示的正文通过工作区提交产生。

## MCP 与 Skill

MCP 提供外部工具。MCP Manager 管理服务器、发现目录和调用权限；Agent 使用已发现的工具目录，调用前由 MCP 服务确认权限。较长结果存为 `tool-results/` 下的可读文件，模型收到摘要和文件路径。配置与连接行为见 [MCP](../CurrentState/MCP.md)。

Skill 提供按需读取的工作方法、材料和脚本。它沿用相同的工具调用路径，知识包本身的组织方式见 [Skill](Skill.md)。

## 添加一个工具

先看相近的内置工具，实现它的参数描述和执行逻辑，再在 `BuiltinAgentToolRegistry` 注册。工具返回内容供模型继续工作；需要文件修改、提交或任务控制时，通过现有 `AgentToolEffect` 交给 runtime 记录和处理。

需要网络、文件或脚本引擎等外部能力时，沿用仓库的 port 与 adapter 边界。验证应从可观察的调用结果和副作用入手，见 [测试](TestingStrategy.md)。

- [registry.rs](../../src-tauri/crates/tt-application/src/services/agent_tools/registry.rs)：内置目录。
- [policy.rs](../../src-tauri/crates/tt-application/src/services/agent_tools/policy.rs)：Invocation 工具快照。
- [tool_request_gate.rs](../../src-tauri/crates/tt-application/src/services/tool_request_gate.rs)：调用检查与预算。
- [tool_execution.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/tool_execution.rs)：分派和记录。
