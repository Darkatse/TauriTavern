# TauriTavern Extension APIs

**为扩展开发者提供 SillyTavern 从未内置的核心能力。**

TauriTavern 专属 API 统一入口：`window.__TAURITAVERN__.api.chat`

### ✨ 亮点

- **`findLastMessage()`** — 一行代码定位最后一条特定消息，告别手动 `reverse().find()`
- **`searchMessages()`** — 内置全文检索，CJK 优化，替代手写关键词扫描
- **`store.*`** — 真正的 per-chat 扩展数据持久化，数据不再塞进消息体
- **`metadata.*`** — 标准化的轻量配置/进度存储

### 文档

| 文档 | 内容 |
| --- | --- |
| [Chat.md](Chat.md) | API 完整参考——接口签名、参数、返回值 |
| [Migration.md](Migration.md) | 适配指南——从 SillyTavern 扩展快速适配到 TauriTavern |
