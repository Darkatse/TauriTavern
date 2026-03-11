# 当前现状说明

本目录用于记录 **已经落地** 的模块现状，而不是方案讨论或未来规划。

它解决的是一个很具体的问题：

> 当我们要继续开发某个模块时，首先需要知道系统现在实际上怎样工作、边界在哪、哪些约束不能轻易打破。

因此，本目录下的文档应保持简短，并优先回答以下问题：

1. 当前模块解决了什么问题
2. 端到端链路现在如何工作
3. 哪些能力已经支持，哪些明确不支持
4. 后续开发时最容易误改的契约是什么

## 与其他文档目录的分工

- `docs/CurrentState/`：当前实现快照与持续开发约束

## 当前条目

1. `docs/CurrentState/ThirdPartyExtensions.md`
   - 第三方前端扩展兼容的当前状态
   - 包含前端加载链路、后端资源端点、目录语义与开发约束

2. `docs/CurrentState/MobileStyleAdaptation.md`
   - 移动端样式适配现状（edge-to-edge / safe-area / 沉浸模式 / 第三方浮层兜底）
   - 包含 Android 原生注入链路、CSS 变量契约、前端消费与回归要点

3. `docs/CurrentState/FrontendPerfBaseline.md`
   - Perf HUD 导出的前端性能基线（启动指标 / invoke 热点 / long frames/tasks）
   - 用于 Phase 1–3 重构的对比与回归门槛
