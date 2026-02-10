# TauriTavern 开发文档

本文档总结了将SillyTavern从Node.js后端迁移到Tauri+Rust后端的主要改动，以及前后端通信和构建系统的改进。

## 最新更新

### 实现ClientVersion API
- 在后端实现了`get_client_version` API，提供版本信息
- 在前端实现了对应的调用逻辑，支持Tauri环境
- 修复了版本显示问题，现在显示为TauriTavern

## 架构概览

TauriTavern采用了Tauri v2作为应用框架，使用Rust作为后端语言，保留了SillyTavern的前端代码。整体架构遵循清晰的模块化设计和关注点分离原则。

### 后端架构 (src-tauri/)

后端采用Rust语言实现，使用Tauri v2 API进行前后端通信。主要组件包括：

1. **核心模块**：
   - 实现了SillyTavern原有的核心功能
   - 采用Clean Architecture设计模式
   - 各模块之间通过明确的接口进行通信

2. **API层**：
   - 使用Tauri命令API暴露后端功能给前端
   - 实现了与原SillyTavern API兼容的接口
   - 添加了详细的日志记录，便于调试

3. **文件系统处理**：
   - 使用Tauri的文件系统API处理文件操作
   - 实现了与原SillyTavern相同的文件结构和权限模型

4. **配置管理**：
   - 使用Tauri的配置API管理应用配置
   - 保持与原SillyTavern配置文件格式兼容

### 前端架构

前端保留了SillyTavern的原有代码，但进行了一些必要的调整以适应Tauri环境：

1. **库加载系统**：
   - 使用webpack打包第三方库
   - 实现了动态库加载机制，解决了模块导入问题
   - 精简了重复的库加载逻辑

2. **前后端通信**：
   - 使用Tauri API替代了原有的HTTP请求
   - 实现了事件监听机制，处理后端事件

3. **UI调整**：
   - 适配了Tauri窗口环境
   - 保持了原有的UI设计和用户体验

## 前后端通信

TauriTavern使用Tauri v2的通信API实现前后端通信，主要包括：

1. **命令调用**：
   - 前端通过`invoke`方法调用后端命令
   - 后端通过`#[tauri::command]`注解暴露API
   - 支持异步操作和错误处理

2. **事件系统**：
   - 后端通过`emit_to`发送事件到前端
   - 前端通过`listen`方法监听后端事件
   - 实现了实时通知和状态更新

3. **文件访问**：
   - 使用Tauri的文件系统API进行文件操作
   - 实现了安全的文件访问控制

## Webpack改进

为了解决库加载问题和优化构建过程，对webpack配置进行了以下改进：

1. **库打包**：
   - 使用webpack打包所有第三方库到单一bundle
   - 替换了原有的mock对象，使用真实库实现

2. **模块化加载**：
   - 实现了`lib-bundle.js`：导入并打包所有库
   - 实现了`lib.js`：直接导入`src/dist/lib.bundle.js`并统一导出
   - 在`init.js`中按顺序加载`lib.js -> tauri-main.js -> script.js`

3. **构建优化**：
   - 配置了生产环境优化
   - webpack直接输出到`src/dist`，避免二次复制
   - 启用了webpack filesystem cache，提升增量构建速度

## 文件结构

主要文件及其功能：

```
src-tauri/          # Rust后端代码
  ├── src/          # Rust源代码
  │   ├── main.rs   # 应用入口点
  │   ├── api/      # API层实现
  │   ├── core/     # 核心功能模块
  │   └── utils/    # 工具函数
  ├── Cargo.toml    # Rust依赖配置
  └── tauri.conf.json # Tauri配置

src/                # 前端代码
  ├── lib-bundle.js # 库打包文件
  ├── lib.js        # 库导出模块
  ├── init.js       # 应用初始化入口
  └── dist/         # webpack输出目录
  └── types.d.ts    # TypeScript类型定义
```

## 调试与日志

为了便于调试和问题排查，添加了详细的日志记录：

1. **前端日志**：
   - 库加载过程的详细日志
   - 前后端通信的请求和响应日志

2. **后端日志**：
   - 使用Rust的日志系统记录详细信息
   - 按模块和级别分类日志
   - 支持将日志写入文件

## 已知问题与限制

1. 某些依赖于Node.js特定功能的扩展可能需要额外适配
2. 文件系统权限模型与原版略有不同，需要适应Tauri的安全模型
3. 某些网络功能受Tauri安全策略限制，需要特殊处理

## 未来改进计划

1. 进一步优化Rust后端性能
2. 改进错误处理和用户反馈机制
3. 添加更多Tauri特有功能，如系统托盘集成
4. 优化应用打包和分发流程

## 参考资源

- [Tauri官方文档](https://tauri.app/v2/docs/)
- [Rust编程语言](https://www.rust-lang.org/)
- [SillyTavern项目](https://github.com/SillyTavern/SillyTavern)
