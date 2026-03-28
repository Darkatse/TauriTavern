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
- `api.dev`
  - 面向调试、诊断与开发工具。
  - 提供前端日志、后端日志、LLM API 日志的统一宿主入口。
- `api.worldInfo`
  - 面向角色卡作者与世界书相关扩展。
  - 提供最近一次激活结果、实时订阅与 best-effort 条目跳转。

## 文档

| 文档 | 内容 |
| --- | --- |
| [Chat.md](Chat.md) | `api.chat` 完整参考 |
| [Dev.md](Dev.md) | `api.dev` 完整参考 |
| [WorldInfo.md](WorldInfo.md) | `api.worldInfo` 完整参考 |
| [Migration.md](Migration.md) | 从 SillyTavern 扩展迁移到 TauriTavern 的适配指南 |

## 契约说明

- API 类型定义见 `src/types.d.ts`
- 宿主契约与稳定性边界见 `docs/FrontendHostContract.md`
- `docs/TauriTavernHostExtensionApiPlan.md` 记录了设计意图，但请以本目录和 Host Contract 为准
