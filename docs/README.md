# TauriTavern 项目文档

本文件夹包含TauriTavern项目的完整文档，用于指导开发和维护工作。

## 文档目录

1. [产品需求文档 (PRD)](./PRD.md) - 详细描述项目的功能需求和目标
2. [技术栈文档](./TechStack.md) - 列出项目使用的技术栈和依赖
3. [前端指南](./FrontendGuide.md) - 前端代码结构、Tauri注入启动链与模块化路由开发指南
4. [前端宿主契约](./FrontendHostContract.md) - Host Kernel 对上游/插件/脚本可观察行为的契约清单（重构必读）
5. [后端结构](./BackendStructure.md) - 后端架构和模块说明
6. [实施计划](./ImplementationPlan.md) - 项目实施的阶段和里程碑
7. [Android 端开发说明](./AndroidDevelopment.md) - Android WebView/Insets 注入、资源访问与路径解析方案
8. [iOS 端开发说明](./iOSDevelopment.md) - WKWebView 行为差异、safe-area/viewport-fit 与底部死区修复
9. [iOS 通用导出桥实现计划](./IosGenericExportBridgePlan.md) - iOS 上将浏览器式导出统一接入原生 Share Sheet 的实施方案
10. [现状说明](./CurrentState/README.md) - 当前实现状态快照与持续开发约束
11. [扩展 API 文档](./API/README.md) - `window.__TAURITAVERN__.api.*` 的参考与适配指南（面向扩展作者）
12. [TauriTavern-Creator-Extension 实现计划](./TauriTavernCreatorExtensionPlan.md) - 面向样板扩展的整体架构、分层、阶段路线与 ROI 分析
13. [宿主扩展 API 规范化计划](./TauriTavernHostExtensionApiPlan.md) - TauriTavern 本体应新增/整理的正式 ABI 设计与落地顺序
14. [Workshop 回调认证重构计划](./WorkshopAuthRefactorPlan.md) - 桌面端 100% 兼容 window.open 弹窗回调，移动端优先覆盖 token/轮询类
15. [Dev Logging 性能重构计划书](./DevLoggingPerformanceRefactorPlan.md) - 日志捕获/存储/展示链路的有界化与性能治理路线图

## 项目概述

TauriTavern是SillyTavern的Tauri重构版本，旨在通过Tauri和Rust重写后端，同时保留原有前端，实现多平台原生应用支持，不再强制依赖Node.js环境。

## 文档维护

这些文档应随着项目的发展而更新，确保它们始终反映项目的当前状态和目标。

当前前端文档已基于 SillyTavern 1.16.0 同步后的模块化注入架构更新。
其中：

- `docs/CurrentState/` 记录“当前已经落地的实现状态”和后续维护约束
