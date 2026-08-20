# TauriTavern Agent Detail Docs

本目录保存 Agent 框架的细节设计文档。高层入口仍保留在 `docs/` 根目录：

1. `docs/AgentArchitecture.md`：系统边界、分层、数据流。
2. `docs/AgentContract.md`：不可破坏的不变量与 fail-fast 约束。
3. `docs/AgentImplementPlan.md`：当前实施基线、下一步计划与验收命令。

## 细节文档

| 文档 | 内容 |
| --- | --- |
| [Workspace.md](Workspace.md) | Workspace、Artifact 与 commit 语义 |
| [RunEventJournal.md](RunEventJournal.md) | Run Event、状态机、订阅、恢复、取消与审批 |
| [ProfilesAndPreset.md](ProfilesAndPreset.md) | Agent Profile、Preset agent schema、ContextFrame、Plan Policy |
| [PromptAssembly.md](PromptAssembly.md) | Agent 独立 Preset / 独立 Model、FrozenRunInputSnapshot 与前后端 prompt assembly 链路 |
| [ToolSystem.md](ToolSystem.md) | ToolId、Catalog、Snapshot、request projection、执行结果、Policy 与 Legacy ToolManager 边界 |
| [SubAgent.md](SubAgent.md) | return-mode SubAgent、AgentInvocation / AgentTask、shared workspace 语义与开发定位 |
| [Handoff.md](Handoff.md) | `agent.handoff` 接力流程、TransferControl task、Handoff invocation、prompt brief 与代码定位 |
| [LlmGateway.md](LlmGateway.md) | provider-agnostic LLM gateway 与现有 `ChatCompletionService` 的复用边界 |
| [McpSkill.md](McpSkill.md) | MCP 独立集成、Skill 渐进披露、安全边界 |
| [Skill.md](Skill.md) | 当前 Skill 格式、存储、导入导出、Agent tool 与安全边界 |
| [SkillScript.md](./SkillScript.md) | `skill.run_script` 脚本开发指南：context 快照、文件读写边界、模块导入与脚本工具箱 |
| [TestingStrategy.md](TestingStrategy.md) | Domain/Application/Frontend/Security/Performance 测试矩阵 |

## 进度跟踪

实时开发进度不写在本目录；请更新 `docs/CurrentState/AgentFramework.md`。

当前已落地 canonical model IR、provider native metadata 保真、invocation-scoped provider state、canonical Tool Control Plane、上下文与 workspace 工具循环、Agent Profile、独立 Preset/Model、return-mode SubAgent 和 foreground handoff。真实能力边界以 `docs/CurrentState/AgentFramework.md` 为准；专题文档只解释各自契约，不单独维护进度副本。
