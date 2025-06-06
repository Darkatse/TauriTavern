# TauriTavern 产品需求文档 (PRD)

## 1. 项目概述

### 1.1 背景

SillyTavern是一个流行的AI聊天前端，目前使用Node.js作为后端。这种架构要求用户安装Node.js环境，增加了使用门槛，并限制了在移动设备和其他平台上的部署。

### 1.2 项目目标

TauriTavern旨在通过Tauri和Rust重写SillyTavern的后端，同时保留原有前端，实现以下目标：

- 提供跨平台原生应用体验（Windows, macOS, Linux, iOS, Android）
- 消除Node.js依赖，降低使用门槛
- 提高应用性能和安全性
- 保持与原SillyTavern前端和生态系统的兼容性
- 支持离线使用和本地部署
- 支持移动端运行

### 1.3 目标用户

- 现有SillyTavern用户
- 希望在不同设备上使用AI聊天前端的用户
- 对安装和配置Node.js环境感到困难的非技术用户
- 需要在资源受限设备上运行应用的用户

## 2. 功能需求

### 2.1 核心功能

TauriTavern将逐步实现SillyTavern的所有核心功能，包括但不限于：

1. **角色管理**
   - 创建、编辑和删除角色，使用PNG格式存储角色卡片
   - 导入/导出角色卡片，保持与SillyTavern生态兼容
   - 角色图像管理，支持裁剪和调整
   - 角色标签和分类系统

2. **聊天功能**
   - 创建和管理聊天会话，使用JSONL格式存储聊天记录
   - 支持单角色和群组聊天，包括多角色互动
   - 聊天历史记录保存、加载和搜索
   - 消息编辑、重新生成和格式化

3. **群组管理**
   - 创建、编辑和删除角色群组
   - 配置群组成员和互动规则
   - 多种角色激活策略（自然、列表、手动、池化）
   - 群组聊天历史管理

4. **用户界面**
   - 保留原SillyTavern的UI设计和用户体验
   - 支持主题和自定义外观，与SillyTavern主题兼容
   - 自定义背景壁纸管理
   - 响应式设计，适应不同屏幕尺寸

5. **系统功能**
   - 用户设置和偏好管理
   - API密钥安全存储和管理
   - 数据备份和恢复
   - 应用更新和扩展管理

### 2.2 Tauri特有功能

利用Tauri框架提供的特性，TauriTavern实现以下额外功能：

1. **系统集成**
   - 跨平台资源访问系统
   - 原生文件系统操作
   - 系统通知与托盘集成

2. **安全性**
   - 安全的API密钥存储
   - 细粒度权限控制

3. **性能优化**
   - Rust后端高性能处理
   - 减少资源消耗
   - 优化启动时间

4. **离线与移动端功能**
   - 完全离线运行能力
   - 移动端支持 (iOS/Android)

## 3. 非功能需求

### 3.1 性能要求

- 应用启动时间应小于3秒
- UI响应时间应小于100ms
- 内存使用应优于原SillyTavern
- 支持在低配置设备上流畅运行

### 3.2 兼容性要求

- 支持Windows 10+, macOS 10.15+, 主流Linux发行版
- 支持iOS 15+, Android 10+
- 保持与SillyTavern扩展和插件的兼容性
- 支持导入/导出SillyTavern数据

### 3.3 安全要求

- 所有API密钥和敏感数据应安全存储
- 目前暂不引入数据加密
- 应用应遵循最小权限原则
- 数据应存储在用户可控的位置

### 3.4 可用性要求

- 界面应保持与SillyTavern一致，减少学习成本
- 应提供详细的错误信息和故障恢复机制
- 应支持多语言

## 4. 约束与限制

### 4.1 技术约束

- 使用Tauri v2和Rust构建后端
- 前端代码保持原SillyTavern代码结构
- 使用模块化适配器连接前后端
- 采用资源系统确保跨平台兼容性

### 4.2 业务约束

- 应保持与SillyTavern相同的开源许可
- 应明确标识为SillyTavern的衍生项目
- 应尊重原项目的贡献者

## 5. 未来扩展

### 5.1 潜在功能

- 本地语音合成和识别
- 增强的离线功能
- 本地向量数据库集成

### 5.2 技术演进

- 逐步优化Rust后端架构
- 探索WebGPU加速
- 考虑使用WASM扩展功能

## 6. 验收标准

TauriTavern将被视为成功，如果它：

1. 实现了SillyTavern的所有核心功能
2. 在所有目标平台上稳定运行
3. 不需要Node.js环境
4. 性能和资源使用优于原SillyTavern
5. 能够无缝导入现有SillyTavern数据
6. 用户反馈积极，迁移障碍低
