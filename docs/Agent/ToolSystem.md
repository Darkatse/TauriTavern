# 工具

工具把模型的请求转成一次有结果的操作。模型读取工具描述，发出调用；runtime 检查本次 Invocation 的工具集合，执行操作，再将结果送回下一轮上下文。

## 身份与调用名称

工具目录使用稳定的 `ToolId`。内置工具是 `builtin:<name>`，MCP 工具是 `mcp/<registration-id>:<name>`，扩展工具是 `extension/<extensionId>:<name>`。Profile 的工具配置和运行记录使用这些 ID。

模型调用名称在 Invocation 创建时生成，与稳定 ID 的映射保存在工具快照中。内置工具如 `workspace_read_file` 保留固定名称；扩展使用注册的 `name`，MCP 使用服务器与工具名，按模型协议规范字符和长度，重名时加 `__2` 等短后缀。

工具准备按目录、Chat/Session 场景和 Profile 选择生成 bindings，Invocation 再单独应用完成协议，形成包含描述、参数、名称映射及预算的快照。输出修订沿用原快照。每个 Invocation 的调用计数独立，由 `ToolRequestGate` 检查后明确分派到 Builtin、MCP 或 Extension。

新 Profile 默认启用 Shell。

## 内置工具

下表使用源码中的工具名。参数以 [descriptor 定义](../../src-tauri/crates/tt-application/src/services/agent_tools) 和 `api.agent.tools.list()` 返回的 schema 为准。

| 工作 | 工具 |
| --- | --- |
| 读取聊天 | `chat.search`、`chat.read_messages` |
| 读取激活世界书 | `worldinfo.read_activated` |
| 读取工作文件与 Skill | `workspace.list_files`、`workspace.search_files`、`workspace.read_file` |
| 修改文件 | `workspace.write_file`、`workspace.apply_patch` |
| Shell 与数据处理 | `workspace.shell`，内含 jq、Python、JavaScript 与 `builtin.` 命令 |
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

Shell 内可用 `builtin.<工具名>` 调用当前 Invocation 可见的内置工具，参数为单个 JSON 对象：

```sh
builtin.chat.search '{"query":"lantern","limit":10}'
builtin.dice.roll '{"formula":"2d6"}'
```

命令集合在 Invocation 编译时确定，等于该次调用的可见工具集减去下表所列的排除项；未注册的名字即普通 `command not found`。成功时工具文本写入 stdout 并以 0 退出，失败时错误信息写入 stderr 并以非零退出，因此 `&&`、`||`、`if` 可直接使用。命令名带 `builtin.` 前缀，避免与 Bashkit 内置命令重名。

**当前内置工具里可以这样调用的有六个**（Profile 未收窄时的全集）：

| 命令 | 用途 |
| --- | --- |
| `builtin.chat.search` | 搜索当前聊天消息 |
| `builtin.chat.read_messages` | 按索引或范围读取聊天消息 |
| `builtin.worldinfo.read_activated` | 读取本次运行激活的世界书 |
| `builtin.dice.roll` | 掷骰 |
| `builtin.workspace.list_files` | 列出可见工作区文件 |
| `builtin.workspace.search_files` | 搜索可见工作区文件 |

**不注册的有十个**，因此它们在 Shell 里不存在：

| 命令 | 不注册的理由 |
| --- | --- |
| `builtin.workspace.shell` | 自我递归 |
| `builtin.workspace.read_file` | 需要模型回合的 CAS 读取记录 |
| `builtin.workspace.write_file` | 同上 |
| `builtin.workspace.apply_patch` | 同上 |
| `builtin.workspace.commit` | 控制流，会破坏 Run 状态机 |
| `builtin.workspace.finish` | 同上 |
| `builtin.agent.delegate` | 委派协议 |
| `builtin.agent.await` | 同上 |
| `builtin.agent.handoff` | 同上 |
| `builtin.task.return` | 同上 |

被 Profile 的 `tools.allow`/`tools.deny` 收窄的工具同样不会注册，与模型侧可见性一致。

### 调用 MCP

Shell 内用三个固定命令访问 MCP。**不是**每个 MCP 工具注册一个命令：MCP 工具集是动态的，而服务器的标识（本机生成的 UUID、用户可改的显示名）都不适合当命令名。

```sh
mcp.list                                              # 列出可见服务器与工具
mcp.check --server "我的工具" search fetch           # 断言这 N 个工具都在
mcp.invoke --server "我的工具" search '{"query":"x"}' --timeout 20
```

**服务器选择器**（三者互斥，必给其一）：

| 选择器 | 解析依据 |
| --- | --- |
| `--server` | 同时按显示名和 registration id 匹配 |
| `--name` | 只按显示名 |
| `--id` | 只按 registration id |

**命中数必须恰好为 1**：找不到会列出可用服务器；匹配到多个（含「某服务器的名字恰好等于另一服务器的 id」）会报错并列出候选，**不会静默选一个**。位置参数只放工具名，服务器始终通过标志给出。

`mcp.check` 要求至少一个工具名，全部齐全才以 0 退出；失败时报明缺哪些，并列出该服务器实际可用的工具。

三个命令都读本次 Invocation 的**冻结快照**（与模型可见集合一致），`mcp.list` 与 `mcp.check` 不发网络请求。

**退出码**：

| 码 | 含义 |
| --- | --- |
| 0 | 成功 |
| 1 | 调用失败（请求未发出、服务器拒绝或工具报错）；前者可安全重跑 |
| 124 | 超过 `--timeout`；**远端可能已执行，重跑前请先确认状态** |
| 2 | 命令行用法错误 |

`--timeout` 必须小于 Shell 的 30s 预算：Shell 会在 30s 掐断整条命令，更长的等待会被报成 Shell 超时而不是本次调用超时，因此超限值直接被拒绝。

服务器可用性沿用既有规则（`Active`，且工具权限不为 `Off`），与模型侧一致。

## 结果如何进入下一轮

`AgentToolResult` 包含调用 ID、工具 ID、文本、结构化结果、错误信息和资源引用。模型读取其中的 `content`，错误结果带有明确的错误标记；结构化元数据保留在记录中，不自动展开为模型文字。

面向模型的文字各司其职：提示词说明职责与完成方式，工具说明用途与参数，结果报告事实，错误说明问题与已知的纠正办法。以理解成本衡量简洁，保留必要的内容边界和后续指引；内部配置、身份与审计细节留在记录中。

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
