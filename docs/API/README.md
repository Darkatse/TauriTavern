# TauriTavern Extension APIs

TauriTavern 专属扩展 API 的统一入口是 `window.__TAURITAVERN__.api`。

这套 API 的目标不是把上游内部实现直接摊给扩展，而是把真正值得长期承诺的平台能力，整理成小而稳定的宿主 ABI。

## 入口

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);

const host = window.__TAURITAVERN__;
const api = host?.api;
```

## API 分区

- `api.chat`
  - 面向记忆类 / 数据库 / 检索类扩展。
  - 提供跨窗口聊天访问、全文检索、per-chat store、metadata、历史分页等能力。
- `api.layout`
  - 面向移动端 UI/面板/悬浮窗/iframe 等需要对齐 safe-area/viewport/IME 的扩展。
  - 提供布局契约快照与订阅，并配合 `data-tt-mobile-surface` taxonomy 实现少量 opt-in 即稳定适配。
- `api.dev`
  - 面向调试、诊断与开发工具。
  - 提供前端日志、后端日志、LLM API 日志的统一宿主入口。
- `api.worldInfo`
  - 面向角色卡作者与世界书相关扩展。
  - 提供最近一次激活结果、实时订阅与 best-effort 条目跳转。
- `api.extension.store`
  - 面向需要**全局持久化**的扩展（不绑定 chat）。
  - 提供 Extension KV JSON + Blob 存储，支持多 table。
- `api.agent`（已落地 Phase 2C 上下文只读工具与 workspace 读改工具循环）
  - 面向 Agent Run / timeline / workspace / checkpoint / commit。
  - 当前提供启动 run、订阅/读取事件、取消、读取 workspace 文件、prepare/finalize/commit；模型侧内建 chat search/read、world info read 与 workspace list/read/write/patch/finish 工具由 Rust runtime 注册。
  - approval、listRuns、readDiff、rollback 仍是后续阶段；当前入口显式 throw。
- `api.mcp`（规划中）
  - 面向 MCP server/tool/resource/prompt 的独立平台能力。
  - Agent 可以消费 MCP，但 MCP 不依附 Agent Mode。

## 文档

| 文档 | 内容 |
| --- | --- |
| [Chat.md](Chat.md) | `api.chat` 完整参考 |
| [Layout.md](Layout.md) | `api.layout` 完整参考（safe-area/viewport/IME） |
| [Dev.md](Dev.md) | `api.dev` 完整参考 |
| [WorldInfo.md](WorldInfo.md) | `api.worldInfo` 完整参考 |
| [ExtensionStore.md](Extension.md) | `api.extension.store` 完整参考 |
| [Agent.md](Agent.md) | `api.agent` 草案（Agent Run / workspace / timeline） |
| [MCP.md](MCP.md) | `api.mcp` 草案（MCP server/tool/resource/prompt） |
| [Migration.md](Migration.md) | 从 SillyTavern 扩展迁移到 TauriTavern 的适配指南 |

## 契约说明

- API 类型定义见 `src/types.d.ts`
- 宿主契约与稳定性边界见 `docs/FrontendHostContract.md`
- Agent 已落地当前 Host ABI、Phase 2C 上下文只读工具与 workspace 读改工具；真实边界见 `docs/API/Agent.md` 与 `docs/CurrentState/AgentFramework.md`
- MCP 仍处于规划阶段；实现前请以 `docs/API/MCP.md` 与 `docs/FrontendHostContract.md` 的草案约束为准
