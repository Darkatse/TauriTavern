# Frontend Host Contract（TauriTavern）

> 目的：把“宿主平台层（Host Kernel）对外承诺的行为”显式化，避免重构 `src/tauri/main/*` 时误伤上游 SillyTavern / 第三方扩展 / 重脚本 / 角色卡。  
> 范围：仅覆盖前端宿主层（WebView 内运行的 Host Kernel）对外可观察的契约；不描述 Rust 后端内部实现。  
> 参考：`docs/FrontendGuide.md`（集成架构与开发方式）

---

## 1. 稳定性分级（写清楚“哪些能改，哪些不能随便改”）

为了避免“什么都是 API”，本仓库把前端宿主行为按稳定性分为 3 类：

1. **Public Contract（对上游/插件/脚本/角色卡承诺）**
   - 一旦变更，必须在本文件记录，并在 smoke tests 里验证（见第 6 节）。
2. **Project Contract（项目内部约定）**
   - 例如 `init.js` 与 `bootstrap.js` 之间的协调信号；可以演进，但需要同步更新相关模块与文档。
3. **Internal（实现细节）**
   - 可自由重构，但不得改变 Public Contract 的外部可观察行为。

---

## 2. 启动链路与就绪信号（Public + Project）

### 2.1 启动顺序（事实）

当前启动链路（见 `docs/FrontendGuide.md`）：

1. `src/init.js`：负责最早期的环境标记、可选 perf 开关与动态 import。
2. `src/tauri-main.js`：薄入口，仅调用 `bootstrapTauriMain()`。
3. `src/tauri/main/bootstrap.js`：composition root，创建 context、注册 routes、安装拦截器与补丁。
4. `src/script.js`：上游 SillyTavern 主应用入口（vendor）。

### 2.2 就绪信号（Public/Project）

- `window.__TAURITAVERN_MAIN_READY__ : Promise<void>`
  - 由 `src/tauri/main/bootstrap.js` 写入，表示宿主层初始化已完成（或失败已被捕获并写入 console）。
- `window.__TAURITAVERN_PERF_READY__ : Promise<unknown> | undefined`
  - 仅在 perf-hud 启用时存在（见第 5 节）。
- `globalThis.__TAURITAVERN_PERF_ENABLED__ : boolean`
  - 由 `src/init.js` 在动态 import 前写入；`bootstrap` 会优先读取它（避免重复计算/时序差异）。
- `window.__TAURI_RUNNING__ : true`
  - 由 `src/init.js` 写入；用于桥接层尽早判断 Tauri 环境（避免移动端注入时序 race）。

---

## 3. 全局 API（Public）

> 这些符号被第三方脚本/扩展/角色卡直接调用，变更需极度谨慎。

### 3.1 资源与缩略图（Public）

由 `src/tauri/main/context.js` 安装：

- `window.__TAURITAVERN_THUMBNAIL__(type, file, useTimestamp?) -> string`
  - 生成缩略图 URL（通常返回 `/thumbnail?...` 或 asset protocol URL）。
- `window.__TAURITAVERN_THUMBNAIL_BLOB_URL__(type, file, options?) -> Promise<string>`
  - 返回可直接用于 `<img src>` 的 blob URL（内部有 cache/in-flight 去重）。
- `window.__TAURITAVERN_BACKGROUND_PATH__(file) -> string`
- `window.__TAURITAVERN_AVATAR_PATH__(file) -> string | null`
- `window.__TAURITAVERN_PERSONA_PATH__(file) -> string`

这些 API 的**可观察行为**必须保持：

- 对同一输入的 URL 形态（路径/查询参数意义）保持一致；
- 失败时的返回值语义保持一致（例如 `null` vs 抛错 vs fallback string）；
- 不得引入同步阻塞（第三方会在渲染路径高频调用）。

### 3.2 Android 导入/导出 Picker（Public）

由 `src/tauri/main/context.js` 安装（用于 Android Content URI 的回调接收）：

- `window.__TAURITAVERN_IMPORT_ARCHIVE_PICKER__`（对象：用于接收 Android 侧回调并 resolve/reject pending promise）
- `window.__TAURITAVERN_EXPORT_ARCHIVE_PICKER__`（同上）

> 这两者属于“跨语言桥接回调点”，命名与行为应视为 Public Contract。

### 3.3 返回键处理（Public）

由 `src/tauri/main/back-navigation.js` 安装：

- `window.__TAURITAVERN_HANDLE_BACK__() -> boolean`
  - 返回 `true` 表示已消费返回键（例如关闭对话框/浮层/抽屉/聊天等），否则返回 `false`。

### 3.4 原生分享桥（Public）

由 `src/tauri/main/share-target-bridge.js` 安装：

- `window.__TAURITAVERN_NATIVE_SHARE__ = { push(payload), subscribe(handler) }`
  - `push()`：注入分享 payload（url 或 png）。
  - `subscribe()`：订阅消费；若早到则进入 backlog，首次订阅会 drain backlog。

---

## 4. 请求拦截与路由契约（Public）

### 4.1 拦截范围（事实）

由 `src/tauri/main/interceptors.js` 安装：

- patch `window.fetch`
- patch `jQuery.ajax`（兼容 jqXHR/Deferred 行为）

拦截生效条件（见 `src/tauri/main/bootstrap.js`）：

- 仅在 **Tauri 环境**启用（`bootstrapTauriMain()` 早退保护）。
- 仅拦截 **same-origin** 请求（包含被 patch 的同源 iframe/window）。
- 是否接管由 `router.canHandle(method, pathname)` 决定（仅看 `url.pathname`）。

### 4.2 未命中行为（Public）

- `fetch`：未命中路由直接透传原生 fetch。
- `ajax`：未命中路由直接透传原始 `$.ajax`。
- 命中但无 handler：返回 `404` JSON（`{ error: "Unsupported endpoint: ..." }`）。

> 这类行为会被上游与第三方依赖：不要改成 silent fail/空响应。

### 4.3 路由表（Public）

路由定义集中在 `src/tauri/main/routes/*`，其路径本身属于 Public Contract（上游/插件会直接请求）。

最关键的启动依赖：

- `/csrf-token`：返回固定 token（用于兼容上游初始化对 CSRF 的假设）
- `/version`：返回版本信息

高频与高风险路径（示例，不是完整列表）：

- `/api/*`：应用核心 API（settings/chats/characters/ai/worldinfo…）
- `/thumbnail`：缩略图/资源路由（与 `__TAURITAVERN_THUMBNAIL__` 强耦合）
- `/user/files/*`、`/User Avatars/*`：用户文件与头像资源访问（含通配符路由）

---

## 5. 兼容补丁与观测（Public/Project）

### 5.1 Perf HUD（Project，作为验收工具）

- 开关：
  - `localStorage.setItem('tt:perf','1')` 后 reload
  - 或 URL 参数 `?ttPerf=1`
- 全局对象：
  - `window.__TAURITAVERN_PERF__`（见 `src/tauri/main/perf/perf-hud.js`）

### 5.2 移动端运行时兼容（Public in practice）

移动端旧 WebView 的 polyfills 与 overlay safe-area 兜底属于“运行环境的一部分”，第三方会依赖其存在：

- `window.__TAURITAVERN_MOBILE_RUNTIME_COMPAT__`
- `window.__TAURITAVERN_MOBILE_OVERLAY_COMPAT__`

---

## 6. Smoke Tests（Public 回归用例）

这些用例是“最小但真实”的兼容回归集（来源：你提供的 `.cache` 样本）：

1. **JS-Slash-Runner**
   - 能加载、UI 能打开、至少一条命令可执行。
2. **database_script**
   - 能注入运行（至少不崩），其 UI/入口可打开。
3. **V1.72（重型角色卡）**
   - iframe 能加载且不被同源 patch/拦截破坏。

任何涉及第 3/4 节契约的改动，都必须至少跑通以上 smoke tests。

