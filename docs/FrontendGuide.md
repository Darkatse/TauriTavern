# TauriTavern 前端指南

本文档描述 TauriTavern 当前前端（基于 SillyTavern 1.16.0）在 Tauri 环境下的集成架构与开发方式。

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
   - 安装同源窗口下载桥（移动端浏览器式导出 -> 原生落盘）
   - 安装 Tauri mobile 兼容层（runtime polyfills + overlay safe-area，仅移动端）
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
│       ├── download-bridge.js # 同源窗口下载桥接
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
- 在 bridge 初始化后再次尝试 patch 运行时补丁（处理加载时序问题）。

### 4.2 `context.js`

- 提供统一的 `safeInvoke`（含短重试机制）。
- 管理角色缓存和名称解析。
- 处理头像/背景等资源路径转换（`convertFileSrc`）。
- 封装表单到 DTO 的转换与上传文件临时落盘。

### 4.3 `interceptors.js`

- 代理 `window.fetch`。
- 代理 `$.ajax` 并保持 Deferred/jqXHR 行为兼容。
- 只拦截本地 API 请求，其余请求透传原生实现。

### 4.4 `download-bridge.js`

- 只处理移动端同源窗口中的浏览器式下载（如 `blob:` / `data:` + `a[download]`）。
- 将命中的导出转接到现有原生文件导出链路。
- 不参与 API 路由判断，避免与请求拦截职责混合。

### 4.5 `router.js` + `routes/*`

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

## 6.1 聊天分段加载（Windowed Loading，Phase 2-C）

- 只在 Tauri 环境启用：以 `isTauriChatPayloadTransportEnabled()` 为准。
- 上游接管点：`src/script.js`（character chat）与 `src/scripts/group-chats.js`（group chat）。
- 统一入口：上游只 import `src/scripts/chat-payload-transport.js`（不要直接依赖 `src/scripts/tauri/chat/*`）。
- window state：`src/scripts/tauri/chat/windowed-state.js`
  - 桌面：`DEFAULT_CHAT_WINDOW_LINES_DESKTOP = 100`
  - 移动端（Android/iOS runtime）：`DEFAULT_CHAT_WINDOW_LINES_MOBILE = 50`
- 初次加载：`load*PayloadTail({ maxLines }) -> { payload, cursor, hasMoreBefore }`。
- 翻页：`load*PayloadBefore({ cursor, maxLines }) -> { messages, cursor, hasMoreBefore }`；prepend 后必须 `updateViewMessageIds(0)`。
- 保存：`save*PayloadWindowed({ cursor, payload }) -> cursor` 并回写 window state；保存前不要落盘 `chat_metadata.lastInContextMessageId`。
- 错误策略：cursor 签名/边界失效直接抛错；不要写“静默回退到全量加载/全量保存”的 fallback。

## 7. 插件系统前端适配

### 7.1 设计目标

- 保持上游 `scripts/extensions.js` 的调用语义不变（manifest 结构、启用逻辑、依赖检查）。
- 将 Tauri 专属逻辑限制在独立 runtime 子模块，减少与上游同步冲突。
- 支持第三方插件从用户数据目录加载 JS/CSS/静态资源，不依赖 Node.js 后端。

### 7.2 模块分层

- `src/scripts/extensions.js`：插件激活编排层（发现、排序、依赖/版本检查、触发加载）。
- `src/scripts/browser-fixes.js`：上游浏览器补丁（保持与 SillyTavern 同步）。
- `src/tauri/main/compat/mobile/mobile-runtime-compat.js`：Tauri mobile 运行时 polyfills（补齐旧 WebView 缺失 JS API）。
- `src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js`：Tauri mobile overlay safe-area top 兜底（遵循当前顶部 safe-area 布局策略）。
- `src/scripts/extensions/runtime/resource-paths.js`：扩展资源路径规范化与 third-party 判定。
- `src/scripts/extensions/runtime/tauri-ready.js`：等待 `__TAURITAVERN_MAIN_READY__`，避免 bridge 未就绪时提前加载。
- `src/scripts/extensions/runtime/third-party-runtime.js`：第三方扩展样式兼容层（仅处理 legacy WebView 的 `@layer` 降级与 `url()` 绝对化；必要时返回 Blob URL，否则返回原始 URL）。
- `src/scripts/extensions/runtime/asset-loader.js`：脚本与样式注入、超时保护、重复注入幂等控制。

### 7.3 端到端加载链路

1. `loadExtensionSettings()` 先等待 `waitForTauriMainReady()`。
2. 前端通过 `/api/extensions/discover` 获取扩展列表与类型，读取 manifest 并进入 `activateExtensions()`。
3. 对每个扩展执行 `addExtensionLocale()` + `addExtensionScript()` + `addExtensionStyle()`。
4. 当扩展为 `third-party/*` 时：
   - JS 入口脚本直接从 `/scripts/extensions/third-party/*` 加载（真实同源静态资源端点）。
   - CSS 仅在旧 WebView 不支持 `@layer` 时经 runtime 预处理为 Blob URL（否则仍走原始 URL）。
5. `/scripts/extensions/third-party/*` 由 Rust 协议层端点提供（WebView `on_web_resource_request` hook），统一返回 bytes + `Content-Type` + 404 语义。

### 7.3.1 当前实现结论

- 当前实现已经从“前端模拟静态文件服务”收敛为“前端只负责编排，Rust 负责 third-party 资源端点”。
- `src/scripts/extensions/runtime/third-party-runtime.js` 不再承担 JS 源码重写或伪服务器职责，主要只保留第三方样式兼容修复。
- 面向持续开发的现状说明见 `docs/CurrentState/ThirdPartyExtensions.md`；涉及实现边界或改动前，先读该文档，再决定是改前端 runtime 还是改后端资源端点。

### 7.4 契约与约束

- third-party 扩展命名约定为 `third-party/<folder>`，前后端均按该约定解析。
- 扩展命令参数统一使用 camelCase（如 `extensionName`），避免 invoke 参数缺失。
- 客户端版本检查仍遵循上游格式：`SillyTavern:<version>:TauriTavern`，用于 `minimum_client_version` 判断。
- 拦截器是否接管请求由 `router.canHandle(method, path)` 决定，不再维护分散的路径白名单。
- `/api/extensions/branches` 与 `/api/extensions/switch` 在 Tauri 后端默认不支持（返回空列表/错误），新增分支能力需后端先实现。

### 7.5 常见问题定位

- `Extension module is not JavaScript`：
  - 通常表示拿到了 HTML 回包而非模块文件。
  - 优先检查 `/scripts/extensions/third-party/*` 是否被协议层端点正确响应（应返回 404 或 JS bytes，而不是 `index.html`）。
- `missing required key extensionName`：
  - 表示 invoke 参数命名不匹配，检查路由 body -> 命令参数映射。
- `stylesheet preprocessing timed out`：
  - 卡在第三方 CSS 预处理阶段，需检查样式资源可达性与 WebView 环境（是否触发 `@layer` 降级分支）。

### 7.6 后续开发规则

- 新增插件加载能力时，优先扩展 `src/scripts/extensions/runtime/*`，不要把 Tauri 细节回灌到 `extensions.js`。
- 新增插件 API 时，优先在 `src/tauri/main/routes/extensions-routes.js` 封装，再通过 `context.safeInvoke()` 调 Rust 命令。
- 若调整 third-party 静态资源路径约定，必须同时更新 `resource-paths.js` 与 Rust 协议层端点的前缀解析逻辑。

### 7.7 移动端插件兼容（新增）

#### 7.7.1 JS 运行时兼容（Android 旧 WebView）

- 实现位置：`src/tauri/main/compat/mobile/mobile-runtime-compat.js`。
- 入口：`src/tauri/main/bootstrap.js` 中安装（仅 Tauri mobile）。
- 行为：仅补齐缺失 API，且只执行一次。
- 当前按需补齐：
  - `Array.prototype.at`
  - `String.prototype.at`
  - `Array.prototype.findLast`
  - `Array.prototype.findLastIndex`
  - `Array.prototype.toSorted`
  - `Array.prototype.toReversed`
  - `Object.hasOwn`

该策略用于修复移动端第三方插件在初始化阶段出现的 `TypeError: *.at is not a function`。

#### 7.7.2 CSS `@layer` 降级（Android 旧 WebView）

- 实现位置：`src/scripts/extensions/runtime/third-party-runtime.js`（样式加载链路）。
- 触发条件：
  - 样式内容包含 `@layer`；
  - 当前 WebView 不支持 CSS Cascade Layers。
- 处理方式：
  - 通过 `css-tools` 解析 CSS AST；
  - 将 `layer` 规则展平为普通规则；
  - 再生成压缩后的 CSS 文本注入 Blob URL。
- 缓存与性能：
  - 能力检测结果缓存（`supportsCssCascadeLayers`）；
  - 样式结果走现有 `styleBlobCache`，同一文件不重复处理；
  - 支持 `@layer` 的环境走快路径，不做转换。

该策略用于修复移动端插件面板（如 `TH-custom-tailwind`）样式大面积失效导致的布局错乱。

#### 7.7.3 浮层 safe-area 修正（移动端）

- 实现位置：`src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js`。
- 入口：`src/tauri/main/bootstrap.js` 中安装（仅 Tauri mobile）。
- 触发条件：仅处理第三方浮层节点（`position: fixed` 且顶边贴近 0）。
- 处理策略：
  - 观察 `document.body` 直接子节点新增/移除；
  - 对命中元素设置 `top: max(var(--tt-safe-area-top), <原top>) !important`；
  - 明确排除 `body/#sheld/#chat` 等应用核心容器，避免影响主界面布局。
- Android 变量语义：`--tt-safe-area-top` 表示当前布局应避开的有效 inset；非沉浸模式下反映顶部 safe area，沉浸模式下回落为 `0`，因此 overlay patch 不再额外避开顶部状态栏/刘海区域。

该策略用于修复 JS-Slash-Runner 等脚本在运行时注入固定定位弹窗样式时，关闭按钮落入状态栏导致不可点击的问题。

#### 7.7.4 调试建议

- 若看到 `*.at is not a function`：
  - 检查是否为 Tauri mobile 会话，并确认 `window.__TAURITAVERN_MOBILE_RUNTIME_COMPAT__ === true`。
- 若插件样式错乱但 CSS 已成功请求：
  - 优先检查是否命中 `@layer` 降级分支；
  - 关注 `resolveStylesheetUrl()` 是否返回预处理后的 Blob URL。
- 若脚本弹窗贴顶到状态栏：
  - 检查脚本是否通过 `<style>` 或行内 `style` 设置了固定定位顶边；
  - 检查 `window.__TAURITAVERN_MOBILE_OVERLAY_COMPAT__` 是否已安装。

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
