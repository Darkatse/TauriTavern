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

页面脚本运行前，宿主已写入 `window.__TAURITAVERN_HOST__`（见 2.2）。

开发态 HTTP 入口会先由 `scripts/tauri-dev-server.mjs` 注入 `src/dev-sw-bootstrap.js`，仅负责让持久 Service Worker registration 与当前 WebView 会话重新绑定，不参与应用模块初始化。

1. `src/init.js`：负责最早期的环境标记、可选 perf 开关与动态 import。
2. `src/tauri-main.js`：薄入口，仅调用 `bootstrapTauriMain()`。
3. `src/tauri/main/bootstrap.js`：composition root，创建 context、注册 routes、安装拦截器与补丁。
4. `src/script.js`：上游 SillyTavern 主应用入口（vendor）。

### 2.2 就绪信号（Public/Project）

- `window.__TAURITAVERN_MAIN_READY__ : Promise<void>`
  - 由 `src/tauri/main/bootstrap.js` 写入，表示宿主层初始化已完成（或失败已被捕获并写入 console）。
  - 在 Tauri runtime 下，resolve 前必须完成 Rust `BackendReadiness` 等待，确保首批依赖 `AppState` 的命令不靠 “state not managed” 文本重试作为正常控制流。
- `window.__TAURITAVERN_PERF_READY__ : Promise<unknown> | undefined`
  - 仅在 perf-hud 启用时存在（见第 5 节）。
- `globalThis.__TAURITAVERN_PERF_ENABLED__ : boolean`
  - 由 `src/init.js` 在动态 import 前写入；`bootstrap` 会优先读取它（避免重复计算/时序差异）。
- `window.__TAURI_RUNNING__ : true`
  - 由 `src/init.js` 写入；用于桥接层尽早判断 Tauri 环境（避免移动端注入时序 race）。
- `window.__TAURITAVERN_HOST__ : { platform, kind }`（Project）
  - 宿主身份：Rust 按编译目标生成，在页面脚本之前写入主 WebView 的顶层 frame，对象冻结。
  - `platform`：`'windows' | 'macos' | 'linux' | 'android' | 'ios' | 'ohos'`；`kind`：`'desktop' | 'mobile'`，与 Rust 的 `desktop` / `mobile` 编译条件同源。
  - 第一方代码只通过 `src/scripts/util/host-identity.js` 读取，见 `docs/FrontendGuide.md` §2.1。

---

## 3. 全局 API（Public）

`SillyTavern.getContext().macros` 提供宏注册与独立求值，接口和使用范围见[宏求值 API](API/Macros.md)。

`SillyTavern.getContext().messageFormatter` 暴露上游 [MessageFormatter](../src/scripts/message-formatter.js) 单例。同步 hook 在 HTML 净化前运行，可重复执行；单个 hook 失败会报告并隔离。延后扩展初始化完成后，通过 ChatSurface 统一补刷已挂载内容。

> 这些符号被第三方脚本/扩展/角色卡直接调用，变更需极度谨慎。

### 3.1 资源与缩略图（Public）

由 `createTauriMainContext()` 安装（实现：`src/tauri/main/context/index.js`，兼容入口：`src/tauri/main/context.js`）：

- `window.__TAURITAVERN_THUMBNAIL__(type, file, useTimestamp?) -> string`
  - 为 `bg` / `avatar` / `persona` 生成 `/thumbnail?...` Host Resource URL；未知 type 直接抛错。
  - 正常第一方路径不使用 `useTimestamp`；该参数只保留为显式 force/debug cache bust。
- `window.__TAURITAVERN_BACKGROUND_PATH__(file) -> string`
  - 生成 `/backgrounds/<encoded file>` Host Resource URL。

这些 API 的**可观察行为**必须保持：

- 对同一输入的 URL 形态（路径/查询参数意义）保持一致；
- 失败时的返回值语义保持一致（例如 `null` vs 抛错 vs fallback string）；
- 不得引入同步阻塞（第三方会在渲染路径高频调用）。

### 3.3 返回键处理（Public）

由 `src/tauri/main/back-navigation.js` 安装：

- `window.__TAURITAVERN_HANDLE_BACK__() -> boolean`
  - 返回 `true` 表示已消费返回键（例如关闭对话框/浮层/抽屉/聊天等），否则返回 `false`。

### 3.4 原生分享桥（Public）

由 `src/tauri/main/share-target-bridge.js` 安装：

- `window.__TAURITAVERN_NATIVE_SHARE__ = { push(payload), subscribe(handler) }`
  - `push()`：注入分享 payload（url 或 png）。
  - `subscribe()`：订阅消费；若早到则进入 backlog，首次订阅会 drain backlog。

### 3.5 上游库兼容全局（Public）

由 `src/lib.js:initLibraryShims()` 安装：

- `window._ : lodash`
  - SillyTavern 生态中的 third-party 扩展可能把 lodash external 为 `_`，并在 ESM 模块求值阶段直接访问。
  - 该符号必须在 third-party 扩展模块加载前可用；不得依赖 webpack/Rspack 等打包器偶然泄漏全局。

该 ABI 属于 SillyTavern 兼容层，不放入 `window.__TAURITAVERN__.api`。新 TauriTavern 代码仍应从 `src/lib.js` 显式 import `lodash`。

ESM `/scripts/secrets.js` 的 `writeSecret`、`deleteSecret`、`readSecretState`、`rotateSecret`、`renameSecret` 在请求或随后的状态刷新失败时拒绝 Promise，本身不弹通知，由调用者处理并通知。`writeSecret` 写入成功而刷新失败时密钥已经保存，不应因此重写；未指定 `allowEmpty` 时传入空值等同删除。

### 3.6 平台 ABI（Public，新）

为避免未来继续扩散 `window.__TAURITAVERN_*` 零散符号，宿主层额外提供一个**统一出口**：

- `window.__TAURITAVERN__ : { abiVersion, traceHeader, ready, invoke, assets, api, iosPolicy }`
  - `abiVersion: 2`：已发布扩展契约发生破坏性语义变化时递增。
  - `traceHeader: string`：请求追踪 header 名（见 4.4）。
  - `ready: Promise<void> | null`：与 `__TAURITAVERN_MAIN_READY__` 语义一致。
  - `invoke.safeInvoke(...)` / `invoke.flushAll()`：对 `context` invoke 能力的稳定包装。
  - `assets.thumbnailUrl` / `assets.backgroundPath`：对 3.1 中稳定 Host Resource URL helper 的统一引用。
  - `iosPolicy`（Project）：iOS 分发策略的启动快照，见 `docs/CurrentState/iOSPolicy.md`。
  - `api.layout`：读取与订阅已排除原生遮挡的内容视口，见 [API/Layout.md](API/Layout.md)。
  - `api.chat`：TauriTavern 独有的聊天/记忆类扩展 API（聊天摘要、元数据、历史分页、稳定存储、后端定位、纯文本检索）。
    - 详细签名与示例见：`docs/API/Chat.md`。
  - `api.characterCards`：用原生选择器选择本地角色卡（`json/png`），返回标准 `File[]`，交给上游的导入或替换流程。
    - `isNativePickerAvailable()`：所有宿主都为 `true`。
    - `pickFiles(options?: { multiple?: boolean }) -> Promise<File[] | null>`：支持多选；取消返回 `null`，失败抛错。
  - `api.extension.store`：扩展级**全局持久化**（不绑定 chat），提供 KV JSON + Blob，支持多 table 与 Blob 流式读取。
    - 签名、读写边界与示例见 [Extension Store API](API/Extension.md)。
  - `api.db`：本地向量、JSON、文本索引、图和 TQL 数据库。`open` 异步等待可用；NodeId 为整数；签名、索引恢复边界与共享关闭语义见 [Database API](API/Database.md)。
  - `api.dev`：TauriTavern 规范化的开发调试 API。内置 Settings 开发面板与第三方扩展都应消费这一层，而不是直接依赖 Tauri 事件名或 Rust 命令名。
    - `api.dev.frontendLogs`
      - `list(options?: { limit?: number }) -> Promise<FrontendLogEntry[]>`
      - `subscribe(handler) -> Promise<unsubscribe>`
      - `getConsoleCaptureEnabled() -> Promise<boolean>`
      - `setConsoleCaptureEnabled(enabled: boolean) -> Promise<void>`
      - 语义：宿主统一负责“运行时开关 + 持久化设置 + 本地 bootstrap flag”同步；调用方不应再自行读写 `localStorage`。
    - `api.dev.backendLogs`
      - `tail(options?: { limit?: number }) -> Promise<BackendLogEntry[]>`
      - `subscribe(handler) -> Promise<unsubscribe>`
      - 语义：宿主负责共享后端日志流；多个订阅者并存时通过引用计数管理 `enable/disable stream`，不得彼此踩踏。
    - `api.dev.llmApiLogs`
      - `index(options?: { limit?: number }) -> Promise<LlmApiLogIndexEntry[]>`
      - `getPreview(id: number) -> Promise<LlmApiLogPreview>`
      - `getRaw(id: number) -> Promise<LlmApiLogRaw>`
      - `subscribeIndex(handler) -> Promise<unsubscribe>`
      - `getKeep() -> Promise<number>`
      - `setKeep(value: number) -> Promise<void>`
      - 语义：宿主统一负责历史索引、实时索引流与 keep 设置持久化；调用方不应直接操作 `devlog_*` 命令。
    - `api.dev.exportBundle() -> Promise<boolean>`：生成并交付 debug bundle（zip），返回是否已交付，详见 `docs/API/Dev.md`。

`api.dev.*` 的长期契约要求：

- DTO 字段保持 camelCase，新增字段只能做向后兼容扩展。
- `subscribe()` / `subscribeIndex()` 返回的 `unsubscribe` 必须幂等且可安全延迟调用。
- Tauri 事件名 `tauritavern-backend-log` / `tauritavern-llm-api-log` 与命令名 `devlog_*` 属于 Internal 实现细节，不是第三方 Public Contract。

- `api.worldInfo`：TauriTavern 规范化的 World Info / Lorebook 激活与导航 API。
  - `getLastActivation() -> Promise<WorldInfoActivationBatch | null>`
    - 返回最近一次真实生成流程对应的最终激活结果。
    - `null` 仅表示当前会话还没有捕获到任何一次最终激活结果。
  - `subscribeActivations(handler) -> Promise<unsubscribe>`
    - 只推送最终激活结果，不暴露 `WORLDINFO_SCAN_DONE` 的中间循环状态。
    - 不复播历史结果；若需要最近一次结果，应先调用 `getLastActivation()`。
  - `openEntry(ref: { world: string; uid: string | number }) -> Promise<{ opened: boolean }>`
    - Best-effort 导航入口。
    - `opened: true` 表示宿主已成功打开目标世界书并尝试定位到目标条目。
    - `opened: false` 表示目标世界书或条目不存在；其他异常直接抛出，便于调试。

`api.worldInfo` 的 v1 收缩边界：

- 只暴露“最终激活批次”，不直接暴露 `WORLD_INFO_ACTIVATED` / `WORLDINFO_SCAN_DONE` 原始载荷。
- 激活条目 DTO 仅承诺：`world`、`uid`、`displayName`、`constant`、可选 `position`。
- 不把扫描循环控制、预算内部状态、可变中间态对象直接升格为 Public Contract。
- `openEntry()` 必须复用上游 World Info 模块自身的导航能力；宿主 ABI 层不得直接依赖 `#WorldInfo`、`#world_editor_select`、`[uid=\"...\"]` 等 DOM 细节。

- `api.agent`：运行控制、历史、工作区详情与 Profile 管理，见 [Agent API](API/Agent.md)。
  - Chat 与 Session 共用 PromptManager、统一的 Agent snapshot 和 Rust 模型/工具循环；Run 事件与实时投影独立于 SillyTavern 生成事件。
  - Chat 使用稳定聊天身份，宿主提交桥复用聊天保存流程；保存成功或明确失败后释放生成状态。续接保留 Run 身份，分叉创建新聊天身份并复制持久版本。
  - Session 独立于当前角色聊天和写作生成状态，不发出 SillyTavern 聊天/生成事件；目录、共享配置与连续历史由后端管理。Agent Mode 只控制 Chat 的 Agent/Legacy 路由。
  - 扩展工具通过 `tools.register/setEnabled/list` 注册、开关与查询，按 Chat/Session 筛选，共用 Agent 工具执行链路；用法见 [注册扩展工具](API/Agent.md#注册扩展工具)。
  - 运行控制属于 Public Contract；模型回合、任务详情、工具目录和 Timeline 关系是 Project Contract，由对应 API 提供展示 DTO。

- `api.llmConnections`：管理 Profile 引用的模型连接，见 [LLM Connection API](API/LlmConnections.md)。Profile 通过连接 ID 和模型 ID 绑定；Model Target 是界面的配置来源。

- `api.skill`：管理本地知识包的导入、编辑、作用域与导出，见 [Skill API](API/Skill.md)。模型通过只读 `skills/` 工作区视图读取材料，经 `workspace.shell` 执行脚本；安装与替换由管理界面处理。

- `api.mcp`：MCP registration、只读 tool discovery、model-facing description override 与第一方 Manager user test call 的独立平台 API。Agent 与 Legacy generation 已通过内部 application seam 消费 MCP，但 MCP 不依附 Agent Mode，公开 API 仍不提供 raw model-call executor。
  - 当前为实验性的 Project Contract；详细签名见 `docs/API/MCP.md`。
  - 当前暴露 `servers.list/create/update/setState/remove/discover/refresh`、`tools.setPermission`、`tools.setDescriptionOverride` 与 `tools.testCall({ registrationId, nativeName, argumentsJson }, { signal? })`；description override 只改变模型 descriptor 副本，不修改 discovery catalog、权限或执行身份；`update` 可修改名称、endpoint、custom headers 与协议版本；`discover` 读取 application persistent catalog，`refresh` 是唯一强制联网入口；不暴露 raw RPC 或 RMCP session。
  - `testCall` 是第一方 Manager 的 Project Contract：Active registration 上的显式用户调用不受 Off/Ask/Allow 阻止且不修改 permission；typed outcome 区分 `known_response`、`not_sent` 与 `outcome_unknown`，AbortSignal 只停止本地等待，不承诺远端回滚。
  - 同一 WebView 内的 vendor/extension scripts 仍按当前平台 trust model 视为用户授权代码；本 ABI 不声称验证物理点击或隔离 hostile extension。
  - server 新建后总是 Paused；工具缺省 Off。discovery annotations 不构成 authority。
  - Agent/Legacy model exposure 都只读取 application persistent snapshot，并在共享 resolver 应用 registration description override；Agent Profile `tools.toolDescriptions` 随后覆盖同一字段。发送前由 Rust 重查 permission；Legacy MCP 不进入全局 SillyTavern ToolManager、slash commands 或 extension enumeration。
  - 第一方 MCP 管理 UI 是独立内置扩展；它只消费本 API，不把 React 状态升格为平台事实，也不在 TauriTavern Settings 中维护第二入口。

> 注意：`window.__TAURITAVERN__` 是“平台 ABI”，应保持**小而稳定**；不要把内部实现对象整个暴露出去。

### 3.7 ChatSurface participant（Project Contract）

- `window.__TAURITAVERN__.api.chatSurface`
  - 当前是实验性的 Project Contract，尚未作为 Public Contract 稳定发布。
  - 暴露 `protocolVersion: 1`、`isManagedOwnershipRequired()`、`registerParticipant()` 与独立的 `registerContentProcessor()`；ownership query 返回本页已冻结的布尔决策，投影控制器、DOM adapter、内部 revision、admission 预算和虚拟滚动引擎均不外露。
  - 内容处理器在首次投影前注册，异步返回显示 HTML；宿主保存结果供重挂载复用，`registration.refresh()` 显式刷新。participant v1 的同步契约保持不变。
  - participant 必须显式声明协议版本；hook 返回同步 disposer，宿主用 `AbortSignal` 表达 mount/content/runtime 三种真实寿命。
  - mount/remount/content lifecycle 不得伪装为 SillyTavern 消息业务事件。
  - 完整协议与 raw API 接入示例见 `docs/API/ChatSurface.md`。

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

- `/api/*` 整个命名空间由宿主接管：未实现的端点（任意 method）与未返回响应的 handler 都返回 `404` JSON（`{ error: "Unsupported endpoint: <path>" }`，`statusText` 同为该文本）。开发态与生产态一致。
- 其余未命中路由的请求：`fetch` 透传原生 fetch，`ajax` 透传原始 `$.ajax`。

> 这类行为会被上游与第三方依赖：不要改成 silent fail/空响应。生产资产协议会把缺失路径回退为 `200 index.html`，因此 `/api/*` 不得透传。

### 4.3 路由表（Public）

路由位于 `src/tauri/main/routes/*`；公共契约覆盖上游兼容及生态实际依赖的接口。

第一方聊天读写直连内部 transport，不产生兼容路由的 Fetch 请求；外部观察应使用既有业务事件。`/api/chats/get`、`/api/chats/group/get`、`/api/chats/save`、`/api/chats/group/save` 仍供扩展调用。保存成功为 `200 { ok: true }`；integrity 冲突按错误码识别，返回 `400 { error: 'integrity' }`。

聊天 get/save 遵循 [ChatPayload §1.1](CurrentState/ChatPayload.md#11-统一格式底线)。兼容 get 返回保留记录原始 JSON 文本的流式数组（含 header 和全部 swipes），无记录返回空数组。正文错误使 `.json()` / `.text()` 或 jQuery 请求失败，即使 HTTP 状态为 200；取消请求或正文读取会停止后续读取。

`saveMetadata()` / `getContext().saveMetadata()` 只替换 `chat_metadata`，正文保持原字节；消息修改须调用完整保存。调度、初始化与错误处理见 [ChatPayload §3.1](CurrentState/ChatPayload.md#31-metadata-保存)。

启用[历史滑动按需加载](CurrentState/ChatPayload.md#21-历史滑动按需加载)时，`getContext().chat` 的历史候选槽位允许为 null；兼容 get、导出与保存文件保持完整。

世界书 get/edit 直接读写磁盘，保留未知字段和键顺序，不合并编辑缓存；get 缺失文件返回空 `entries`。`WORLDINFO_UPDATED` 仅在保存成功后发送；读取或保存失败阻止依赖该世界书的生成。

最关键的启动依赖：

- `/csrf-token`：返回固定 token（用于兼容上游初始化对 CSRF 的假设）
- `/version`：返回版本信息

高频与高风险路径（示例，不是完整列表）：

- `/api/*`：应用核心 API（settings/chats/characters/ai/worldinfo…）
  - 用户设定沿用上游 settings 数据形状；列表刷新不写入资料，单项保存失败不影响其他修改。
  - `/api/settings/save` 接收完整 JSON。读取与保存确认返回 `tauritavern_settings_revision`；调用方将其作为不透明令牌，以 JSON 编码经 `X-TauriTavern-Settings-Revision` 回传。过期令牌返回 409，未携带时仍可覆盖保存。Persona 修改独立确认成败。
  - `/api/settings/get` 保持上游响应形状，`settings` 仍为 JSON 字符串；启动读取与应用顺序见 [StartupOptimization §4](CurrentState/StartupOptimization.md#4-设置与-bootstrap-元数据读取)。
- `/api/backends/chat-completions/generate` 的流式响应由 Rust 进程内会话持有生成任务、移动端 best-effort 后台执行租约与未确认事件；前端通过单调递增的 `after_seq` 消费并确认，WebView 暂停后可在同一 Rust 进程内重放缺失事件。单次读取失败会使用相同 cursor 重试一次，第二次失败才关闭会话。后台租约失败或到期不拥有生成终止权；该保证不跨进程重启，也不伪装成上游 provider 的 HTTP 断点续传。
- `/css/user.css`：用户自定义 CSS 覆盖文件（数据目录 `_css/user.css`）
- `/scripts/extensions/third-party/*`：third-party 扩展静态资源端点（ESM/CSS/url()/字体/图片）
- `/thumbnail`：缩略图端点（与 `__TAURITAVERN_THUMBNAIL__` 强耦合）
- 用户静态资源端点（通配符路由）：
  - `/characters/*`、`/User Avatars/*`
  - `/backgrounds/*`、`/assets/*`
  - `/user/images/*`、`/user/files/*`

`/api/users/backup` 与上游不同：由宿主生成并交付备份，返回 `{ ok, delivered, includes_secrets }`。

### 4.4 浏览器资源契约（Public）

这些路径必须能被浏览器**原生子资源加载**（`<img src>` / `<link href>` / `<script src>` / `CSS url()`），且 dev/prod 语义一致：

- `/scripts/extensions/third-party/*`
  - `.git` 是 SillyTavern 迁移兼容所需的普通路径组件；显式文件请求不得仅因任一组件名为 `.git` 而被拒绝。`.`、`..`、编码路径分隔符与路径逃逸仍必须拒绝。
- `/css/user.css`
- `/scripts/tauritavern/layout-kit.js`（ESM；`api.layout` 的辅助入口，见 [API/Layout.md](API/Layout.md)）
- `/thumbnail?type={bg|avatar|persona}&file=...`
- `/characters/*`、`/User Avatars/*`
- `/backgrounds/*`、`/assets/*`
- `/user/images/*`、`/user/files/*`

第三方扩展管理 API 保持上游 install/update/version/branches/switch DTO 与 hook/event 语义，remote transport 为 Rust gitoxide smart HTTP。mutating update 不使用前端 timeout/AbortSignal，因为 Tauri invoke 已进入 Rust 后不能据此取消磁盘写入。version 的 UI AbortSignal 仍可用于关闭只读检查结果。branches 只投影唯一远端 heads（空 label，不 fetch object）；switch 成功为 `204`，当前 branch 是 no-op，且不得隐式触发 update hook。

对这些端点的最小可观察语义：

- 仅接受 `GET` / `HEAD` / `OPTIONS`
- 未命中返回真实 `404`（不回退 `index.html`）
- `Content-Type` 正确；成功表示使用 `Cache-Control: private, no-cache`、weak ETag 和适用的 Last-Modified，错误/OPTIONS/416 使用 `no-store`（完整条件请求与平台 delivery 语义见 `docs/CurrentState/HostResourceCaching.md`）
- 媒体文件（`video/*` / `audio/*`）必须支持 `Range`（单范围）并返回 `206 + Content-Range`（见 `docs/CurrentState/MediaAssetContract.md`）

背景预览与背景消费是两个不同表示：

- 系统静态图预览使用普通 `/thumbnail`；
- GIF/WebP/APNG 在预览动画开启时使用 raw `/backgrounds/*`，关闭时使用 `/thumbnail?...&static=true` 的 first-frame JPEG；
- MP4 选择器使用占位图，不为 poster 引入视频解码器；
- active background 与 `<video>` 始终保留 raw `/backgrounds/*`，`static=true` 不得进入播放路径。

禁止事项（为了保持契约稳定）：

- 禁止通过 DOM 原型级 monkey patch（例如改写 `HTMLImageElement.src`）来“模拟”这些端点的加载行为；必须补齐真实端点。

### 4.5 Request tracing（Project，建议作为调试常用工具）

对所有被宿主接管的路由响应，都会附带一个追踪 header：

- `x-tauritavern-trace-id: <traceId>`

用途：将 DevTools Network 中的单次请求，与 console 日志 / perf-hud 数据关联起来，定位第三方脚本导致的异常与性能热点。
header 名也可从 `window.__TAURITAVERN__?.traceHeader` 获取（用于避免硬编码）。

第一方聊天 transport 直连不产生兼容路由 Response，因此没有该响应 header；扩展主动请求兼容路由时仍按上述规则追踪。

---

## 5. 兼容补丁与观测（Public/Project）

### 5.1 Perf HUD（Project，作为验收工具）

`window.__TAURITAVERN_PERF__` 是性能观测入口；启用与报告导出见 [FrontendGuide §10.1](FrontendGuide.md#101-轻量性能仪表perf-hud)。

### 5.2 移动端运行时兼容（Public in practice）

移动端旧 WebView 的 polyfills 属于运行环境兼容能力：

- `window.__TAURITAVERN_MOBILE_RUNTIME_COMPAT__`
  - 覆盖移动端旧 WebView 的基础 polyfills（例如 `requestIdleCallback` / `cancelIdleCallback`）。
  - Android 的 `navigator.clipboard.writeText()` 映射到宿主原生写入器，same-origin iframe 同样适用；Clipboard 对象上的其他方法保持不变。
- `window.__TAURITAVERN_MOBILE_WINDOW_OPEN_COMPAT__`：移动端外链 `window.open()` 通过系统浏览器打开（不创建应用内新窗口）

移动布局：移动端原生宿主消费系统栏、刘海与停靠键盘，网页侧契约见 [API/Layout.md](API/Layout.md)。相关名称的当前状态：

| 名称 | 状态 |
| --- | --- |
| `--doc-height` | 静态别名：`100dvh`，不支持时 `100vh` |
| `--tt-inset-*` | `env(safe-area-inset-*, 0px)` 的别名 |
| `--tt-viewport-bottom-inset` | `--tt-inset-bottom` 的别名 |
| `--tt-window-x/-y/-width/-height` | Project：只写在 `#bg1` 上，供第一方窗口背景使用 |
| `data-tt-mobile-surface`；`layout-kit.js` 的 `SURFACE`/`applySurface()` | 保留，无布局作用 |
| `--tt-ime-bottom`、`--tt-base-viewport-height`、`__TAURITAVERN_INSETS__`、`__TAURITAVERN_MOBILE_OVERLAY_COMPAT__`、`__TAURITAVERN_MOBILE_IFRAME_VIEWPORT_CONTRACT_BRIDGE__`、`__TAURITAVERN_ANDROID_IME_LAYOUT_HOST__` | 已移除 |

TauriTavern 第一方功能在所有平台直接使用同一个原生剪贴板写入器。该契约只授予 `clipboard-manager:allow-write-text`，不包含读取/清空/图片等权限；写入失败必须向调用方传播。

---

### 5.3 Dialog 兼容（Public in practice）

> 目的：补齐 iOS/macOS（WKWebView）下脚本生态高频依赖的“浏览器内置弹窗语义”，避免出现“点击后完全无反应”。

- iOS/macOS：`window.alert/confirm/prompt` 必须可用且不会挂死（无法展示 UI 时返回 Cancel/默认值并记录错误，不做 silent noop）
- 若运行环境缺失 `HTMLDialogElement.prototype.showModal`：宿主会安装 `dialog-polyfill` 并覆盖主窗口 + same-origin iframe/window（仅在缺失时启用）
- 实施细节与边界：见 `docs/WkWebViewJsDialogBridgePlan.md`

### 5.4 外链打开与 `window.open()`（Public in practice）

桌面端（Windows/macOS/Linux）：

- `window.open(url, name, features)`：
  - 若 `features` 指定了 `size/position`（典型 OAuth popup），宿主会在 App 内创建新 WebView 窗口，保持 `window.opener` / `postMessage` 回调语义可用。
  - 其余外链（`http/https/mailto/tel`）默认使用系统浏览器打开（避免在 App 内打开文档/升级链接）。

移动宿主（`kind: 'mobile'`）：

- `window.open()` 不创建应用内新窗口；对外链（`http/https/mailto/tel`）通过系统浏览器打开，并返回 `null`（等价“弹窗被阻止”的可观察语义）。

工程约定（Project）：

- 显式外链打开统一使用 `src/tauri-bridge.js` 的 `openExternalUrl()`；例如 `tauritavern-version` 扩展与自动更新弹窗。

### 5.5 桌面全屏快捷键（Project）

- Windows/macOS/Linux 主窗口使用无修饰键 `F11` 切换原生窗口全屏；按键长按只响应首次 `keydown`。
- 宿主在主文档及其同源 iframe 中捕获该快捷键；独立 popup、移动端和带修饰键的 `F11` 不属于此契约。
- 原生窗口状态是唯一真值；全屏不写入 SillyTavern 设置，也不纳入窗口几何持久化。切换失败会记录错误，后续按键仍可重试。

### 5.6 桌面文件拖放（Public in practice）

- Windows/macOS/Linux 主窗口在创建时关闭 Tauri 原生拖放处理，将文件拖放交给 WebView 的 HTML5 `DataTransfer` / `drop`。
- 由实际落点的前端处理器执行角色卡导入、聊天导入或附件/图库上传；不新增原生文件读取或第二条导入链路。
- 此入口不再产生 Tauri 原生 `tauri://drag-*` 事件；扩展应使用 DOM 拖放事件。
- 文件格式校验、导入提示和持久化继续由原有前端/后端导入流程负责；普通文件选择器入口不变。
- 移动端与独立 popup WebView 的拖放策略不变；主页面内的 HTML 弹窗仍由各自的 DOM 拖放处理器负责。

### 5.7 Android 图片长按保存（Public in practice）

- Android WebView 没有图片上下文菜单。宿主在主文档与同源 iframe 中补上 `<img>` 的 `contextmenu` 默认行为：用户确认后，把这张图片交给系统保存面板，由用户选择保存位置。
- 与浏览器一致，只有 `preventDefault()` 会取消这一默认行为，`stopPropagation()` 不影响。自行处理图片长按的扩展应在 `contextmenu` 上调用 `preventDefault()`。
- 图片按 `currentSrc` 取得原始字节，不重新编码，失败直接提示。`blob:`、`data:` 与同源图片在其所在窗口读取；其他 http(s) 图片与浏览器的“保存图片”一样不受页面 CORS 限制，由宿主重新请求该地址，不带页面的 Cookie 与 Referer。
- 宿主不注册原生长按菜单，长按产生的 `contextmenu` 始终先交给页面处理。

## 6. Smoke Tests（Public 回归用例）

这些用例是“最小但真实”的兼容回归集（来源：你提供的 `.cache` 样本）：

1. **JS-Slash-Runner**
   - 能加载、UI 能打开、至少一条命令可执行（iOS/macOS：`confirm/prompt` 与 `<dialog>.showModal()` 不应 silent noop）。
2. **database_script**
   - 能注入运行（至少不崩），其 UI/入口可打开。
3. **V1.72（重型角色卡）**
   - iframe 能加载且不被同源 patch/拦截破坏。
4. **浏览器资源契约（端点级）**
   - `/thumbnail?type=bg|avatar|persona&file=...` 能返回图片 bytes（无 `blob:` 魔法）；不存在返回真实 `404`
   - `/thumbnail?type=bg&file=<animated>&static=true` 对可解码动画返回 JPEG；MP4 的 raw `/backgrounds/*` Range 播放不受影响
   - `/css/user.css` 能从数据目录 `_css/user.css` 返回用户 CSS；不存在返回真实 `404`
   - `/characters/*`、`/User Avatars/*`、`/backgrounds/*`、`/assets/*`、`/user/images/*`、`/user/files/*` 作为子资源可直接加载
   - `/scripts/extensions/third-party/*` 的 ESM/CSS/图片/字体均可加载，未命中返回 `404`；无秘密 fixture 的 `.git/HEAD` / `.git/config` 采用同一文件级路径语义
   - 媒体 Range 契约：`/backgrounds/<file>.mp4` 的 `Range: bytes=0-1` 返回 `206` 且包含 `Content-Range`
5. **Android 图片长按保存**
   - 长按聊天图片出现保存确认，确认后出现系统保存面板。
   - 没有 CORS 的跨域图片也能保存；地址没有扩展名时，按响应类型补上扩展名。
   - 同源 iframe（包括 srcdoc）中的图片和 `a[download]` 都能保存。
   - 页面对该次 `contextmenu` 调用 `preventDefault()` 时，不出现确认框。

任何涉及第 3/4 节契约的改动，都必须至少跑通以上 smoke tests。

---

## 7. 工程约束（Project，维护者）

> 这些约束不属于第三方“对外 API”，但属于长期维护的硬门槛：它们用于防止宿主层再次退化为单体与隐式耦合。

- Guardrails：`pnpm run check:frontend`（`scripts/check-frontend-guardrails.mjs`）
  - 行数预算：关键聚合文件受 `scripts/guardrails/frontend-lines-baseline.json` 约束。
  - 依赖边界：`kernel/ports` 不得 import `services/routes/adapters`；`services` 不得 import `routes`。
  - 路由契约：`src/tauri/main/routes/*` 禁止直接引用 `window`（通过 `adapters/*` 触碰浏览器/DOM/上游 ST）。
- 类型检查：`pnpm run check:types`（`tsc -p tsconfig.host.json`）
- Invoke surface：宿主层已知命令名集中在 `src/tauri/main/kernel/invokes/tauri-commands.js`（减少字符串漂移与 typo）
