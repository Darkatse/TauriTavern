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
    ├── extensions/runtime/      # 第三方插件运行时（资源解析/模块重写/加载器）
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
2. 拦截器通过 `router.canHandle(method, path)` 判断是否由本地路由接管
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

## 7. 插件系统前端适配

### 7.1 设计目标

- 保持上游 `scripts/extensions.js` 的调用语义不变（manifest 结构、启用逻辑、依赖检查）。
- 将 Tauri 专属逻辑限制在独立 runtime 子模块，减少与上游同步冲突。
- 支持第三方插件从用户数据目录加载 JS/CSS/静态资源，不依赖 Node.js 后端。

### 7.2 模块分层

- `src/scripts/extensions.js`：插件激活编排层（发现、排序、依赖/版本检查、触发加载）。
- `src/scripts/extensions/runtime/resource-paths.js`：扩展资源路径规范化与 third-party 判定。
- `src/scripts/extensions/runtime/tauri-ready.js`：等待 `__TAURITAVERN_MAIN_READY__`，避免 bridge 未就绪时提前加载。
- `src/scripts/extensions/runtime/third-party-runtime.js`：第三方 ESM 重写与 Blob URL 解析（处理动态导入/循环依赖、HTML 误回包检测；请求阶段使用当前 `window.fetch` 以确保命中拦截器）。
- `src/scripts/extensions/runtime/asset-loader.js`：脚本与样式注入、超时保护、重复注入幂等控制。

### 7.3 端到端加载链路

1. `loadExtensionSettings()` 先等待 `waitForTauriMainReady()`。
2. 前端通过 `/api/extensions/discover` 获取扩展列表与类型，读取 manifest 并进入 `activateExtensions()`。
3. 对每个扩展执行 `addExtensionLocale()` + `addExtensionScript()` + `addExtensionStyle()`。
4. 当扩展为 `third-party/*` 时：
   - JS/CSS 先经 runtime 解析为 Blob URL，再注入页面。
   - runtime 拉取依赖时请求 `/scripts/extensions/third-party/*`。
5. `extensions-routes.js` 将该路径转发为 `read_third_party_extension_asset` Tauri 命令，从本地文件系统读取内容并返回 MIME。

### 7.4 契约与约束

- third-party 扩展命名约定为 `third-party/<folder>`，前后端均按该约定解析。
- 扩展命令参数统一使用 camelCase（如 `extensionName`），避免 invoke 参数缺失。
- 客户端版本检查仍遵循上游格式：`SillyTavern:<version>:TauriTavern`，用于 `minimum_client_version` 判断。
- 拦截器是否接管请求由 `router.canHandle(method, path)` 决定，不再维护分散的路径白名单。
- `/api/extensions/branches` 与 `/api/extensions/switch` 在 Tauri 后端默认不支持（返回空列表/错误），新增分支能力需后端先实现。

### 7.5 常见问题定位

- `Extension module is not JavaScript`：
  - 通常表示拿到了 HTML 回包而非模块文件。
  - 优先检查 `/scripts/extensions/third-party/*` 是否命中 `extensions-routes.js`。
- `missing required key extensionName`：
  - 表示 invoke 参数命名不匹配，检查路由 body -> 命令参数映射。
- `script/stylesheet preprocessing timed out`：
  - 卡在第三方依赖预处理阶段，需检查插件依赖图和资源可达性。

### 7.6 后续开发规则

- 新增插件加载能力时，优先扩展 `src/scripts/extensions/runtime/*`，不要把 Tauri 细节回灌到 `extensions.js`。
- 新增插件 API 时，优先在 `src/tauri/main/routes/extensions-routes.js` 封装，再通过 `context.safeInvoke()` 调 Rust 命令。
- 若调整插件路径约定，必须同时更新 `resource-paths.js` 与 `extensions-routes.js` 的路径解析规则。

## 8. 兼容层策略

- `src/tauri-main.js`：新主入口（推荐）。
- 新开发统一集中在 `src/tauri/main/*`，避免重复实现与多处注入链路并存。

## 9. 如何新增一个 Tauri 注入接口

1. 在 Rust 后端新增/确认命令（`src-tauri/src/presentation/commands/*`）。
2. 在 `src/tauri/main/routes/` 对应业务域中新增路由。
3. 路由内只做参数校验、DTO 组装、`context.safeInvoke` 调用。
4. 需要共享逻辑时，优先放到 `context.js` 或 `http-utils.js`，不要回写到单体入口。
5. 保持返回结构稳定（状态码 + JSON 结构），避免破坏上游前端调用假设。

## 10. 调试与验证

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
