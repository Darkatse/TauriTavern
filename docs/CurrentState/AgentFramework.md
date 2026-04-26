# Agent Framework 当前进度

本文档用于后续记录 Agent 框架的实时开发进度。它不是架构设计源文档，也不替代 `docs/AgentArchitecture.md`、`docs/AgentContract.md`、`docs/AgentImplementPlan.md` 与 `docs/Agent/` 下的细节设计。

## 当前状态

截至 2026-04-26：

- Agent Runtime 尚未实现。
- `window.__TAURITAVERN__.api.agent` 尚未实现。
- `window.__TAURITAVERN__.api.mcp` 尚未实现。
- 已完成第一轮架构/契约/实施计划/API 草案文档整理。
- Agent 细节文档已收拢到 `docs/Agent/`。

## 文档入口

- 高层架构：`docs/AgentArchitecture.md`
- 硬契约：`docs/AgentContract.md`
- 实施计划：`docs/AgentImplementPlan.md`
- 细节文档目录：`docs/Agent/README.md`
- Agent API 草案：`docs/API/Agent.md`
- MCP API 草案：`docs/API/MCP.md`

## 进度台账

| 日期 | Phase | 状态 | 变更/PR | 备注 |
| --- | --- | --- | --- | --- |
| 2026-04-26 | Phase 0 | 规划完成 | 文档整理 | 建立 Agent 架构、契约、实施计划、API 草案与细节文档目录 |

## 实施检查表

| 项目 | 状态 | 代码入口 | 测试/验证 | 备注 |
| --- | --- | --- | --- | --- |
| Agent domain models | 未开始 | - | - | `AgentRun` / `AgentRunEvent` / `WorkspacePath` 等 |
| AgentRuntimeService | 未开始 | - | - | 应位于 `src-tauri/src/application/services/agent_runtime/` |
| Workspace repository | 未开始 | - | - | 需先确定物理根目录与同步语义 |
| Journal repository | 未开始 | - | - | append-only JSONL，支持分页 |
| LLM gateway wrapper | 未开始 | - | - | 必须复用 `ChatCompletionService` |
| `api.agent` Host ABI | 未开始 | - | - | 需更新 `src/types.d.ts` |
| 最小 timeline UI | 未开始 | - | - | 不伪装成 SillyTavern `GENERATION_*` 事件 |
| ToolRegistry/ToolDispatch | 未开始 | - | - | Phase 2 |
| `api.mcp` Host ABI | 未开始 | - | - | Phase 5，MCP 独立于 Agent Mode |

## 每次 Agent 相关变更必须更新

新增或修改 Agent 相关实现时，请在本文件补充：

- 当前 phase 与状态变化。
- 涉及的 Rust/前端文件路径。
- 新增或变更的 Host ABI。
- 是否影响 Legacy Generate。
- 是否影响 windowed payload 保存契约。
- 新增测试与验证命令。
- 已知风险和后续待办。

## 守护契约

后续进度记录必须显式关注：

- Agent Mode off 时 Legacy `Generate()` 行为不变。
- LLM 调用不绕过 `ChatCompletionService`、LLM API log、proxy、secret、iOS policy。
- Agent 工具结果不写入 chat 楼层。
- Agent run/timeline event 不伪装成 SillyTavern `GENERATION_*` / `TOOL_CALLS_*` 事件。
- Commit/rollback 遵守 windowed payload 与保存串行化契约。
- MCP stdio command 不由 Agent/Preset/角色卡/世界书直接写入。
