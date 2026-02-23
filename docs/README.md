# TauriTavern 项目文档

本文件夹包含TauriTavern项目的完整文档，用于指导开发和维护工作。

## 文档目录

1. [产品需求文档 (PRD)](./PRD.md) - 详细描述项目的功能需求和目标
2. [技术栈文档](./TechStack.md) - 列出项目使用的技术栈和依赖
3. [前端指南](./FrontendGuide.md) - 前端代码结构、Tauri注入启动链与模块化路由开发指南
4. [后端结构](./BackendStructure.md) - 后端架构和模块说明
5. [实施计划](./ImplementationPlan.md) - 项目实施的阶段和里程碑
6. [移动端开发说明](./MobileDevelopment.md) - Android/iOS 关键坑位、资源访问与路径解析方案
7. [上游 1.16.0 同步计划](./upstream-sync-1.16-plan/README.md) - 上游升级分阶段计划、冲突矩阵与执行清单

## 项目概述

TauriTavern是SillyTavern的Tauri重构版本，旨在通过Tauri和Rust重写后端，同时保留原有前端，实现多平台原生应用支持，不再强制依赖Node.js环境。

## 文档维护

这些文档应随着项目的发展而更新，确保它们始终反映项目的当前状态和目标。

当前前端文档已基于 SillyTavern 1.15.0 同步后的模块化注入架构更新。
