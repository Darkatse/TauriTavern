# TauriTavern Extension APIs

本目录面向 **扩展作者**，描述 TauriTavern 提供的“宿主专属 API”（区别于上游 SillyTavern 的 `getContext()`）。

核心设计点：

- **唯一入口**：`window.__TAURITAVERN__.api.chat`（不做 alias，避免与上游契约混淆）
- **面向 windowed payload**：历史按需读，避免全量 chat 常驻 JS 内存
- **移动端友好**：支持后端定位/检索 + 扫描上限（`scanLimit`）
- **状态持久化落地**：提供 per-chat 的 `metadata` 与 `store` 接口

## 文档列表

- `docs/API/Chat.md`：`window.__TAURITAVERN__.api.chat` API 参考
- `docs/API/Migration.md`：从 SillyTavern 扩展迁移/适配的实战指南（尤其是记忆类扩展）

