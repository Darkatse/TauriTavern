# TauriTavern 技术栈文档

本文档详细描述TauriTavern项目使用的技术栈和依赖，为开发者提供技术选型参考。

## 1. 核心框架

### 1.1 Tauri

- **版本**: Tauri v2
- **用途**: 跨平台应用框架，提供原生功能访问
- **优势**:
  - 比Electron更轻量
  - 更好的安全模型
  - 使用系统WebView而非捆绑Chromium
  - 支持多平台构建
- **文档**: [Tauri官方文档](https://tauri.app/v2/docs/)

### 1.2 Rust

- **版本**: Rust 1.70+
- **用途**: 后端逻辑实现，替代原SillyTavern的Node.js后端
- **优势**:
  - 高性能
  - 内存安全
  - 强类型系统
  - 优秀的并发模型
- **文档**: [Rust官方文档](https://www.rust-lang.org/learn)

### 1.3 前端框架

- **框架**: 保留SillyTavern原有的前端代码
- **核心技术**: HTML, CSS, JavaScript (jQuery)
- **构建工具**: Webpack (用于库打包)

## 2. 后端技术栈

### 2.1 Rust库和框架

| 库/框架 | 用途 |
|---------|------|
| serde | JSON序列化/反序列化 |
| tokio | 异步运行时 |
| reqwest | HTTP客户端 |
| tauri-api | Tauri API访问 |
| rusqlite | SQLite数据库访问（可选） |
| log | 日志记录 |
| env_logger | 日志配置 |
| async-trait | 异步trait支持 |
| thiserror | 错误处理 |

### 2.2 架构模式

- **Clean Architecture**: 采用领域驱动设计和清晰的关注点分离
- **分层架构**:
  - 领域层 (Domain): 核心业务逻辑和实体
  - 应用层 (Application): 用例和服务
  - 基础设施层 (Infrastructure): 外部服务集成和持久化
  - 表示层 (Presentation): API和命令处理

### 2.3 数据存储

- **主要存储**: 文件系统 (JSON文件)
- **数据位置**: 用户数据目录，遵循平台标准

## 3. 前端技术栈

### 3.1 保留的SillyTavern技术

| 技术 | 用途 |
|------|------|
| jQuery | DOM操作和事件处理 |
| Bootstrap | UI组件和响应式布局 |
| Showdown | Markdown渲染 |
| DOMPurify | HTML净化 |
| Highlight.js | 代码高亮 |
| localForage | 客户端存储 |

### 3.2 前端构建工具

- **Webpack**: 用于打包第三方库
- **Babel**: JavaScript转译

### 3.3 前端与后端通信

- **Tauri API**: 使用Tauri提供的IPC机制
- **命令模式**: 通过`invoke`调用Rust后端函数
- **桥接层**: `tauri-bridge.js` 提供 `invoke/listen/convertFileSrc` 统一封装
- **请求注入**: `src/tauri/main/interceptors.js` 同时拦截 `fetch` 与 `jQuery.ajax`
- **路由分发**: `src/tauri/main/router.js` + `src/tauri/main/routes/*` 按业务域分模块处理
- **上下文能力**: `src/tauri/main/context.js` 负责缓存、DTO转换与公共 helper

## 4. 开发工具

### 4.1 IDE和编辑器

- **推荐IDE**:
  - VS Code (前端和Rust)
  - IntelliJ IDEA + Rust插件
  - Rust Analyzer (Rust语言服务器)

### 4.2 构建和包管理

- **Rust**: Cargo
- **前端**: npm/pnpm
- **应用打包**: Tauri CLI

### 4.3 测试工具

- **Rust单元测试**: Rust内置测试框架
- **集成测试**: 自定义测试框架
- **前端测试**: 保留SillyTavern的测试方法

### 4.4 CI/CD

- **GitHub Actions**: 自动构建和测试
- **发布管理**: GitHub Releases

## 5. 第三方服务集成

### 5.1 AI提供商API

| 服务 | 集成方式 | 用途 |
|------|----------|------|
| OpenAI | REST API | GPT模型访问 |
| Anthropic | REST API | Claude模型访问 |
| 本地模型 | HTTP API | 本地AI模型访问 |
| 其他提供商 | 各自API | 多样化模型选择 |

### 5.2 其他服务

- **更新检查**: GitHub API
- **翻译服务**: 保留SillyTavern的集成

## 6. 安全考虑

### 6.1 API密钥管理

- **存储**: 安全存储在用户数据目录
- **访问控制**: 最小权限原则

### 6.2 数据安全

- **本地存储**: 所有数据存储在本地
- **无远程传输**: 除非用户明确配置
- **权限隔离**: 使用Tauri的权限模型

## 7. 性能优化

### 7.1 Rust优化

- **异步处理**: 使用tokio进行异步操作
- **内存管理**: 利用Rust的所有权系统
- **并行处理**: 适当使用并行计算

### 7.2 前端优化

- **资源加载**: 优化资源加载顺序
- **库注入链路**: `lib.js` 直接导入 `src/dist/lib.bundle.js`，移除运行时动态 loader
- **延迟加载**: 非关键资源延迟加载
- **构建缓存**: webpack filesystem cache 提升增量构建速度

## 8. 兼容性和迁移

### 8.1 数据兼容性

- **角色卡片兼容**: 使用与SillyTavern相同的PNG格式角色卡片
- **聊天记录兼容**: 使用与SillyTavern相同的JSONL格式聊天记录
- **群组数据兼容**: 保持与SillyTavern相同的群组数据格式
- **主题兼容**: 支持SillyTavern的主题格式
- **导入/导出支持**: 支持导入/导出SillyTavern数据

### 8.2 API兼容性

- **模块化注入架构**: 使用 `bootstrap/context/interceptors/routes` 拆分原单体入口
- **动态导入模式**: 启动阶段由 `init.js` 统一加载 `tauri-main.js`
- **前端拦截**: 通过 `fetch + jQuery.ajax` 双通道拦截无缝替换HTTP请求
- **路由域隔离**: 系统/设置/扩展/资源/角色/聊天路由分文件维护，降低耦合
- **扩展兼容**: 支持现有扩展系统

## 9. 部署和分发

### 9.1 打包格式

- **Windows**: MSI, EXE
- **macOS**: DMG, App Bundle
- **Linux**: AppImage, DEB, RPM
- **移动平台**: APK, IPA

### 9.2 更新机制

- **自动更新**: 使用Tauri的更新API
- **增量更新**: 可能的增量更新支持
- **版本管理**: 语义化版本控制
