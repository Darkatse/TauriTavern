# 扩展 API

TauriTavern 的宿主 API 位于 `window.__TAURITAVERN__.api`。使用前等待宿主就绪：

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const api = window.__TAURITAVERN__.api;
```

| API | 用途 |
| --- | --- |
| [api.chat](Chat.md) | 聊天读取、检索、metadata 与扩展存储 |
| [api.layout](Layout.md) | safe area、viewport 与输入法布局 |
| [api.dev](Dev.md) | 前后端日志和模型请求诊断 |
| [api.worldInfo](WorldInfo.md) | 世界书激活结果与条目导航 |
| [api.extension.store](Extension.md) | 扩展的全局 JSON / Blob 存储 |
| [api.agent](Agent.md) | 运行控制、历史、文件详情与 Profile 管理 |
| [api.llmConnections](LlmConnections.md) | Agent 使用的模型连接 |
| [api.skill](Skill.md) | Skill 导入、编辑、作用域与导出 |
| [api.mcp](MCP.md) | MCP 服务器、工具发现、权限与测试调用 |

完整类型见 [src/types.d.ts](../../src/types.d.ts)，稳定性约定见 [宿主契约](../FrontendHostContract.md)。从 SillyTavern 扩展迁移时，可参考 [迁移指南](Migration.md)。

框架内部的工作方式见 [Agent](../Agent/README.md)；本目录集中说明调用方法和返回结果。
