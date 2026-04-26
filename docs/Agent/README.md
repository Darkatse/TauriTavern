# TauriTavern Agent Detail Docs

本目录保存 Agent 框架的细节设计文档。高层入口仍保留在 `docs/` 根目录：

1. `docs/AgentArchitecture.md`：系统边界、分层、数据流。
2. `docs/AgentContract.md`：不可破坏的不变量与 fail-fast 约束。
3. `docs/AgentImplementPlan.md`：分阶段实施计划与验收标准。

## 细节文档

| 文档 | 内容 |
| --- | --- |
| [Workspace.md](Workspace.md) | Workspace、Artifact、Checkpoint、commit/rollback 语义 |
| [RunEventJournal.md](RunEventJournal.md) | Run Event、状态机、订阅、恢复、取消与审批 |
| [ProfilesAndPreset.md](ProfilesAndPreset.md) | Agent Profile、Preset agent schema、ContextFrame、Plan Policy |
| [ToolSystem.md](ToolSystem.md) | ToolSpec、ToolResult、Tool Registry、Policy、审批与 Legacy ToolManager 边界 |
| [LlmGateway.md](LlmGateway.md) | provider-agnostic LLM gateway 与现有 `ChatCompletionService` 的复用边界 |
| [McpSkill.md](McpSkill.md) | MCP 独立集成、Skill 渐进披露、安全边界 |
| [TestingStrategy.md](TestingStrategy.md) | Domain/Application/Frontend/Security/Performance 测试矩阵 |

## 进度跟踪

实时开发进度不写在本目录；请更新 `docs/CurrentState/AgentFramework.md`。

截至 2026-04-26，Phase 2A 已落地最小工具循环与前端 dryRun adapter。当前真实能力边界以 `docs/CurrentState/AgentFramework.md` 为准；本目录中的部分内容仍是后续 Phase 2B+ / Phase 3+ 设计。
