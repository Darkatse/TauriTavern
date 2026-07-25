# TauriTavern 技术栈

本文档说明 TauriTavern 当前采用的主要技术、架构约束和工程守护。它不是完整依赖清单；精确版本以 `package.json`、`src-tauri/Cargo.toml` 和各 crate 的 `Cargo.toml` 为准。

## 1. 项目定位

TauriTavern 将 SillyTavern 1.18.0 前端移植到 Tauri v2 原生应用中，并用 Rust 后端替代上游 Node/Express 后端。

核心取舍：

- 保留上游前端体验、数据格式、资源路径和扩展可观察行为。
- 后端采用 Rust workspace crate 拆分，Clean Architecture 是最高层架构约束。
- 前端通过 Tauri 注入层拦截 `fetch` 与 `jQuery.ajax`，把上游同源请求转到本地 Rust 能力。
- 新的 TauriTavern 扩展能力优先暴露在 `window.__TAURITAVERN__.api.*`，不污染上游 `/api/*` 兼容层。

## 2. 基础运行栈

| 技术 | 当前用途 |
| --- | --- |
| Tauri v2 | 原生应用 shell、WebView、插件、跨平台打包、移动端入口 |
| Rust stable / edition 2024 | Rust 后端、workspace crate、native integration |
| Cargo workspace | 后端 crate 边界、依赖方向、focused tests |
| Node.js `>=22.12.0` | 前端工具链与 guard scripts |
| pnpm | JavaScript 包管理与项目脚本 |
| Rspack | 前端 core/optional vendor bundle 构建 |
| TypeScript | Tauri Host Kernel 类型检查 |

## 3. 后端架构

后端以 Clean Architecture 为主轴：越靠近领域语义的代码越靠内，越依赖框架、文件系统、网络、平台 API 的代码越靠外，源码依赖只能从外层指向内层。

当前主要 crate：

| crate | 职责 |
| --- | --- |
| `tauritavern` | Tauri host、commands、composition root、host-bound infrastructure、platform glue |
| `tt-domain` | 领域模型、值对象、领域错误、纯规则 |
| `tt-contracts` | 跨 crate DTO、事件、payload、host resource 契约 |
| `tt-ports` | repository / gateway / runtime trait |
| `tt-application` | use case、service、job coordinator、policy 编排 |
| `tt-adapter-http` | 共享 HTTP client pool/profile/helper |
| `tt-adapter-provider-http` | LLM、SD、Translate、TTS、provider metadata 的 HTTP repository |
| `tt-adapter-tokenization` | tokenizer concrete repository |
| `tt-adapter-storage-core` | `DataDirectory`、基础文件系统 helper、chat/settings/user/theme/secret 等基础存储 |
| `tt-adapter-storage-userdata` | character、world info、agent workspace/profile、skill local package store、PNG card metadata |
| `tt-adapter-media` | avatar/background/user media/image metadata、browser-visible host resource file store |
| `tt-adapter-extension` | third-party extension 发现、安装、版本检查、更新、分支查询/切换、删除与移动；Gitoxide smart HTTP 与 embedded worktree |
| `tt-adapter-sync` | LAN Sync、TT-Sync v2 runtime、client/server、sync jobs |
| `tt-adapter-archive` | data archive import/export executor、archive path safety |

详细边界以 `docs/BackendStructure.md` 为准。

## 4. Rust 依赖分组

| 依赖 | 主要用途 |
| --- | --- |
| `tauri` / Tauri plugins | host shell、文件/通知/打开器/对话框/window state/barcode scanner 等平台能力 |
| `serde` / `serde_json` / `serde_yaml` | DTO、配置、SillyTavern 兼容数据格式 |
| `tokio` / `tokio-util` | 异步任务、文件 IO、取消与运行时能力 |
| `reqwest` / `hyper-util` / `tokio-tungstenite` | provider HTTP、stream、移动端 HTTP client 适配 |
| `gix` / `gix-transport` | third-party extension Git smart HTTP、embedded repository/worktree |
| `tracing` / `tracing-subscriber` / `tracing-appender` | 后端日志、过滤、rolling file、Dev observability |
| `thiserror` | 分层错误类型 |
| `async-trait` | repository / gateway async trait |
| `miktik` | tokenizer 计数与编解码 |
| `image` / `mime_guess` | 角色卡、媒体、图片 metadata |
| `zip` / `tar` / `flate2` / `async-compression` | skill、archive、Dev bundle、压缩数据处理 |
| `axum` / `axum-server` / `ttsync-*` | LAN Sync / TT-Sync v2 |
| `uuid` / `chrono` / `rand` | 标识、时间、随机值 |

当前主存储不是 SQLite。用户数据以 SillyTavern 兼容文件布局为主，必要的 TauriTavern 私有状态放在 `_tauritavern` 下。

## 5. 前端技术栈

前端主体来自 SillyTavern 1.18.0，保留其 HTML/CSS/JavaScript 组织方式和上游依赖生态。

常见技术：

| 技术 | 用途 |
| --- | --- |
| jQuery / jQuery UI | DOM、事件、legacy UI 行为 |
| Bootstrap | UI 组件和响应式布局 |
| Handlebars | 模板渲染 |
| Showdown | Markdown 渲染 |
| DOMPurify | HTML 净化 |
| Highlight.js | 代码高亮 |
| localForage | 浏览器侧存储 |
| Vue | 本项目前端扩展新增引入 |

TauriTavern 自己维护的前端集成层位于 `src/tauri/main/*`，按 `context/kernel/services/adapters/routes` 拆分：

- `context` 暴露稳定 Host Kernel facade 和类型。
- `kernel` 放纯逻辑，例如策略、追踪、键生成、格式化。
- `services` 放有状态能力。
- `adapters` 触碰 `window`、DOM 或上游 SillyTavern 对象。
- `routes` 处理被 host 接管的同源请求。

## 6. 前后端通信

兼容流量的主要链路：

```text
SillyTavern frontend / extension / script
  -> same-origin fetch / jQuery.ajax
  -> src/tauri/main/interceptors.js
  -> src/tauri/main/routes/*
  -> context.safeInvoke(...)
  -> tauritavern presentation command
  -> tt-application service
  -> tt-ports trait
  -> tt-adapter-* concrete implementation
```

维护原则：

- `/api/*` 只承载上游 SillyTavern 兼容行为。
- TauriTavern 新能力优先走 `window.__TAURITAVERN__.api.*`。
- Rust command 名是内部实现细节，不是扩展作者的稳定 API。
- 浏览器子资源使用真实 URL/Response 语义，不用 IPC/base64 伪装资源加载。

## 7. 数据与兼容性

TauriTavern 的数据兼容目标是浏览器、扩展和用户数据可观察的 SillyTavern 语义，而不是 Node/Express 内部实现。

必须保持稳定的内容包括：

- `default-user`、`characters`、`chats`、`group chats` 等目录语义。
- `User Avatars`、`QuickReplies`、`OpenAI Settings`、`TextGen Settings` 等大小写和空格。
- 聊天 JSONL、角色卡 PNG metadata、世界书、预设、主题、用户目录等格式。
- 第三方扩展资源路径和 host resource 响应语义。

数据目录选择与当前状态见 `docs/CurrentState/DataDirectorySelection.md`。

## 8. 工程守护

常用入口：

```bash
pnpm run check
pnpm run check:frontend
pnpm run check:types
pnpm run check:contracts
pnpm run check:rust-boundaries
pnpm run test:rust:split-crates
pnpm run test:rust:host-resources
pnpm run check:rust:dev
```

架构相关 guard：

- `scripts/check-rust-crate-boundaries.mjs` 守住 Rust crate 依赖方向。
- `scripts/check-frontend-guardrails.mjs` 守住前端注入层边界。
- `scripts/check-logging-boundaries.mjs` 守住 logging target 使用边界。
- `tsconfig.host.json` 为 Host Kernel 提供 TypeScript 检查。

## 9. 平台与分发

桌面：

- Windows：MSI / EXE / portable
- macOS：DMG / App Bundle
- Linux：AppImage / DEB / RPM，以及从源码构建的 Nix flake

移动端：

- Android：Tauri Android 工程与 WebView/Insets 适配，见 `docs/AndroidDevelopment.md`
- iOS：WKWebView、safe area、policy 与 native glue，见 `docs/iOSDevelopment.md`

便携模式通过 `TAURITAVERN_RUNTIME_MODE=portable` 或 `portable.flag` 启用。

## 10. 相关文档

| 主题 | 文档 |
| --- | --- |
| 后端 Clean Architecture 与 crate 边界 | `docs/BackendStructure.md` |
| 前端集成结构 | `docs/FrontendGuide.md` |
| Host ABI / 请求拦截 / 资源契约 | `docs/FrontendHostContract.md` |
| 当前实现状态 | `docs/CurrentState/README.md` |
| Linux 分发与 Nix | `docs/CurrentState/LinuxRepository.md` |
| 扩展 API | `docs/API/README.md` |
| Agent 架构 | `docs/AgentArchitecture.md` |
