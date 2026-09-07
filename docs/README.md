# TauriTavern 项目文档

TauriTavern 保留 SillyTavern 的前端体验与扩展生态，用 Tauri 和 Rust 后端提供本地存储、模型调用及平台能力。

## 开始阅读

先看 [项目哲学](../CONTRIBUTING.md#项目哲学)，了解项目如何处理兼容性、复杂度和维护成本。随后按工作方向选择入口：

| 方向 | 文档 |
| --- | --- |
| 仓库技术栈与开发检查 | [技术栈](TechStack.md) |
| Rust 服务与 crate 边界 | [后端结构](BackendStructure.md) |
| 前端启动、路由与集成 | [前端指南](FrontendGuide.md) |
| 前端和扩展可观察的宿主行为 | [宿主契约](FrontendHostContract.md) |
| 编写扩展 | [扩展 API](API/README.md) |
| 理解或修改 Agent | [Agent](Agent/README.md) |
| 查找某个模块的实现 | [模块文档](CurrentState/README.md) |
| Android / iOS 开发 | [Android](AndroidDevelopment.md)、[iOS](iOSDevelopment.md) |

Agent 文档从文件、工作区、循环和日志讲起，再说明多 Agent 如何通过任务与结果协作。配置、工具、脚本和 API 可按需要继续查阅。

## 维护文档

每个主题保留一个主要入口，正文说明用途、工作方式和源码位置。接口用法放在 API 文档，字段与默认值引用源码。行为变化时更新所属主题，删除已经过时的说明。
