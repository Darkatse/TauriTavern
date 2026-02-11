# TauriTavern 前端指南

本文档描述 TauriTavern 当前前端（基于 SillyTavern 1.15.0）在 Tauri 环境下的集成架构与开发方式。

## 1. 目标与原则

- **最小侵入**：尽量保持上游 SillyTavern 前端行为不变。
- **模块化**：将 Tauri 注入逻辑拆分为独立模块，避免单文件膨胀。
- **低耦合**：路由注册、请求拦截、业务上下文分离。
- **入口收敛**：统一走 `init.js -> tauri-main.js -> tauri/main/*`，减少重复入口。

## 2. 启动链路

当前前端启动顺序如下：

1. `src/init.js` 动态导入：`lib.js` -> `tauri-main.js` -> `script.js`
2. `src/lib.js` 静态导入 `src/dist/lib.bundle.js`，统一提供 ESM 导出
3. `src/tauri-main.js` 仅调用 `bootstrapTauriMain()`（薄入口）
4. `src/tauri/main/bootstrap.js` 负责：
   - 创建运行上下文（`context`）
   - 注册前端路由（`router + routes/*`）
   - 安装请求拦截器（`fetch` 与 `jQuery.ajax`）
   - 初始化 bridge 与目录信息

## 3. 目录结构（前端集成相关）

```text
src/
├── tauri-bridge.js            # 低层 bridge：invoke/listen/convertFileSrc
├── tauri-main.js              # 新入口：只做 bootstrap
├── tauri/
│   └── main/
│       ├── bootstrap.js       # 组合根（composition root）
│       ├── context.js         # 状态与共享业务能力
│       ├── http-utils.js      # URL/Body/Response 工具
│       ├── interceptors.js    # fetch/jQuery 注入
│       ├── router.js          # 轻量路由注册与分发
│       └── routes/
│           ├── system-routes.js
│           ├── settings-routes.js
│           ├── extensions-routes.js
│           ├── resource-routes.js
│           ├── character-routes.js
│           ├── chat-routes.js
│           └── ai-routes.js
└── scripts/
    └── ...                    # 上游 SillyTavern 功能模块
```

## 4. 核心模块职责

### 4.1 `bootstrap.js`

- 组装模块依赖并执行初始化。
- 确保只 bootstrap 一次。
- 在 bridge 初始化后再次尝试 patch jQuery（处理加载时序问题）。

### 4.2 `context.js`

- 提供统一的 `safeInvoke`（含短重试机制）。
- 管理角色缓存和名称解析。
- 处理头像/背景等资源路径转换（`convertFileSrc`）。
- 封装表单到 DTO 的转换与上传文件临时落盘。

### 4.3 `interceptors.js`

- 代理 `window.fetch`。
- 代理 `$.ajax` 并保持 Deferred/jqXHR 行为兼容。
- 只拦截本地 API 请求，其余请求透传原生实现。

### 4.4 `router.js` + `routes/*`

- `router.js` 提供简洁注册接口：`get/post/all`。
- `routes/*` 按业务域组织，降低文件复杂度与改动冲突。

## 5. 请求注入流程

1. 前端发起 `fetch('/api/...')` 或 `$.ajax('/api/...')`
2. 拦截器判断是否属于需要接管的本地接口
3. 命中后交给路由分发到 `routes/*`
4. 路由通过 `context.safeInvoke(...)` 调用 Rust 命令
5. 返回标准 `Response` 给前端调用方

补充：`/csrf-token` 在 `system-routes.js` 中返回固定 token，用于通过前端初始化流程中的 CSRF 依赖检查。

## 6. 路由分域说明

| 文件 | 负责范围 |
|------|----------|
| `system-routes.js` | ping/version/csrf 等系统基础接口 |
| `settings-routes.js` | 设置、快照、密钥、预设 |
| `extensions-routes.js` | 扩展发现、安装、更新、删除等 |
| `resource-routes.js` | 头像、背景、主题、群组等资源接口 |
| `character-routes.js` | 角色列表、创建、编辑、导入导出、重命名 |
| `chat-routes.js` | 聊天读写、搜索、最近记录、导出 |
| `ai-routes.js` | Chat Completion（OpenAI / Claude / Gemini(MakerSuite)）与 tokenizer（count/encode/decode/bias） |

## 7. 兼容层策略

- `src/tauri-main.js`：新主入口（推荐）。
- 已移除 legacy 入口文件（`tauri-init.js` / `tauri-integration.js` / `tauri/*-adapter.js`）。
- 新开发统一集中在 `src/tauri/main/*`，避免重复实现与多处注入链路并存。

## 8. 如何新增一个 Tauri 注入接口

1. 在 Rust 后端新增/确认命令（`src-tauri/src/presentation/commands/*`）。
2. 在 `src/tauri/main/routes/` 对应业务域中新增路由。
3. 路由内只做参数校验、DTO 组装、`context.safeInvoke` 调用。
4. 需要共享逻辑时，优先放到 `context.js` 或 `http-utils.js`，不要回写到单体入口。
5. 保持返回结构稳定（状态码 + JSON 结构），避免破坏上游前端调用假设。

## 9. 调试与验证

建议最小验证流程：

1. `pnpm run build`
2. `pnpm run tauri:dev`
3. 启动后确认：
   - 首屏加载正常
   - 不再出现 CSRF 初始化错误
   - 角色/聊天/设置等核心接口可用

如需快速定位问题：

- 查看 DevTools 中请求是否命中本地注入路径。
- 查看控制台 `invoke` 报错信息与路由返回状态码。
- 检查对应 `routes/*` 是否遗漏请求字段映射。
