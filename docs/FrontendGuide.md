# TauriTavern 前端指南

本文档提供TauriTavern前端代码的结构说明和开发指南，帮助开发者理解和扩展前端功能。

## 1. 前端概述

TauriTavern的前端基本保留了SillyTavern的原始代码，仅进行了必要的修改以适应Tauri环境。前端主要使用HTML、CSS和JavaScript (jQuery)构建，没有使用现代前端框架如React或Vue。

### 1.1 设计原则

- **最小修改原则**: 尽可能少地修改原SillyTavern前端代码
- **兼容性优先**: 确保与SillyTavern扩展和主题兼容
- **渐进增强**: 在保持基本功能的同时，逐步增强Tauri特有功能

## 2. 目录结构

```
src/
├── assets/           # 静态资源（图片、字体等）
├── css/              # 样式文件
├── dist/             # 打包后的库文件
├── scripts/          # JavaScript模块
│   ├── extensions/   # 扩展功能
│   ├── kai-settings/ # 设置相关
│   ├── openai/       # OpenAI集成
│   └── ...
├── lib/              # 第三方库
├── index.html        # 主HTML文件
├── script.js         # 主JavaScript文件
├── style.css         # 主样式文件
├── tauri-api.js      # Tauri API封装
├── tauri-bridge.js   # Tauri通信桥接
└── types.d.ts        # TypeScript类型定义
```

## 3. 关键文件说明

### 3.1 核心文件

| 文件 | 描述 |
|------|------|
| `index.html` | 应用主HTML文件，包含UI结构 |
| `script.js` | 主JavaScript文件，包含核心逻辑 |
| `style.css` | 主样式文件 |
| `tauri-bridge.js` | 前端与Rust后端通信的桥接层 |
| `tauri-integration.js` | Tauri环境集成和初始化 |
| `lib-bundle.js` | 打包的第三方库 |
| `lib.js` | 库导出模块，提供统一接口 |

### 3.2 功能模块

| 目录/文件 | 描述 |
|-----------|------|
| `scripts/extensions/` | 扩展系统相关代码 |
| `scripts/kai-settings/` | 设置管理相关代码 |
| `scripts/openai/` | OpenAI API集成 |
| `scripts/secrets.js` | API密钥管理 |
| `scripts/characters.js` | 角色管理 |
| `scripts/chat.js` | 聊天功能 |
| `scripts/power-user.js` | 高级用户功能 |
| `scripts/templates.js` | 模板渲染系统 |

### 3.3 Tauri适配器

| 目录/文件 | 描述 |
|-----------|------|
| `scripts/tauri/` | Tauri API适配器目录 |
| `scripts/tauri/secrets-api.js` | 密钥管理API适配器 |
| `scripts/tauri/extensions-api.js` | 扩展管理API适配器 |
| `scripts/tauri/templates-api.js` | 模板系统API适配器 |
| `tauri/` | Tauri适配器初始化目录 |
| `tauri/extensions-adapter.js` | 扩展系统适配器初始化 |
| `tauri/templates-adapter.js` | 模板系统适配器初始化 |

## 4. Tauri集成

### 4.1 通信机制

TauriTavern使用Tauri的IPC机制替代了SillyTavern的HTTP API调用。主要通信方式包括：

1. **命令调用**: 使用`invoke`函数调用Rust后端函数
2. **事件监听**: 使用`listen`函数监听后端事件
3. **文件系统访问**: 使用Tauri的文件系统API

### 4.2 前端适配模式

TauriTavern采用了一种优雅的前端适配模式，使原有SillyTavern代码能够在Tauri环境中运行：

```javascript
// 示例：动态导入Tauri API
// 检查是否在Tauri环境中运行
const isTauri = window.__TAURI_INTERNALS__ !== undefined;

// 动态导入Tauri API
let tauriAPI = null;
if (isTauri) {
    import('./tauri/module-api.js').then(module => {
        tauriAPI = module;
        console.log('Tauri API loaded');
    }).catch(error => {
        console.error('Failed to load Tauri API:', error);
    });
}

// 在函数中使用Tauri API
async function someFunction() {
    if (isTauri && tauriAPI) {
        // 使用Tauri API
        return await tauriAPI.someFunction();
    } else {
        // 回退到HTTP API
        const response = await fetch('/api/endpoint');
        return response.json();
    }
}
```

### 4.3 模块化API适配器

TauriTavern为每个功能模块提供专门的API适配器，位于`scripts/tauri/`目录：

```javascript
// 示例：扩展API适配器 (extensions-api.js)
import { createApiClient } from './api-client.js';

// 创建API客户端
const extensionsApi = createApiClient('extensions');

/**
 * 获取所有扩展
 * @returns {Promise<Array>} - 扩展列表
 */
export async function getExtensions() {
    return extensionsApi.call('get_extensions');
}

/**
 * 安装扩展
 * @param {string} url - 扩展仓库URL
 * @param {boolean} global - 是否全局安装
 * @returns {Promise<Object>} - 安装结果
 */
export async function installExtension(url, global = false) {
    return extensionsApi.call('install_extension', { url, global });
}
```

## 5. 库加载系统

TauriTavern实现了一个动态库加载系统，解决了在Tauri环境中加载第三方库的问题：

### 5.1 lib-bundle.js

包含所有打包的第三方库，使用webpack构建。

### 5.2 lib-loader.js

负责动态加载库bundle并将库暴露到全局作用域。

### 5.3 lib.js

从全局作用域获取库并提供统一的导出接口。

## 6. 前端修改指南

### 6.1 修改原则

1. **保持兼容性**: 修改不应破坏与原SillyTavern的兼容性
2. **隔离Tauri代码**: Tauri特有代码应适当隔离，便于维护
3. **详细注释**: 所有修改应有详细注释说明

### 6.2 添加新功能

添加新功能时，应遵循以下步骤：

1. 在`tauri-bridge.js`中添加与后端通信的基础函数
2. 在`tauri-api.js`中添加高级API封装
3. 修改相关前端模块，使用新API
4. 添加适当的错误处理和回退机制

### 6.3 修改现有功能

修改现有功能时，应遵循以下步骤：

1. 识别需要修改的功能点
2. 检查是否涉及后端API调用
3. 如果涉及，使用Tauri API替代HTTP调用
4. 保留原有逻辑作为回退机制

### 6.4 调试技巧

1. 使用`console.log`进行基本调试
2. 使用Tauri的开发工具检查通信
3. 使用浏览器开发者工具调试前端
4. 添加详细的日志记录

## 7. 常见问题与解决方案

### 7.1 库加载问题

**问题**: 第三方库无法正确加载或报错"Failed to resolve module specifier"

**解决方案**:
- 确保库已正确打包到`dist/lib.bundle.js`
- 检查`lib-loader.js`中的路径配置
- 使用全局变量而非ES模块导入

### 7.2 通信问题

**问题**: 与后端通信失败

**解决方案**:
- 检查Tauri环境是否正确初始化
- 确认后端命令已正确注册
- 添加详细的错误处理和日志

### 7.3 文件访问问题

**问题**: 无法访问文件系统

**解决方案**:
- 确认Tauri配置中已允许相应的文件系统权限
- 使用Tauri的文件系统API而非Web API
- 添加适当的错误处理

## 8. 性能优化建议

1. **延迟加载**: 非关键资源应延迟加载
2. **减少DOM操作**: 批量处理DOM操作
3. **缓存数据**: 适当缓存频繁访问的数据
4. **优化事件监听**: 使用事件委托减少监听器数量
5. **减少IPC调用**: 批量处理后端请求

## 9. 测试指南

### 9.1 手动测试

1. 测试基本功能是否正常工作
2. 测试在有/无网络环境下的行为
3. 测试错误处理和恢复机制
4. 测试与原SillyTavern的兼容性

### 9.2 自动化测试

可以考虑添加以下自动化测试：

1. 单元测试: 测试独立功能模块
2. 集成测试: 测试模块间交互
3. E2E测试: 测试完整用户流程

## 10. 贡献指南

1. 遵循项目的代码风格和命名约定
2. 提交前进行充分测试
3. 提供详细的提交信息
4. 更新相关文档
5. 考虑兼容性和性能影响
