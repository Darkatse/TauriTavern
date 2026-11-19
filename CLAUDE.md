# CLAUDE.md — TauriTavern

TauriTavern：把 [SillyTavern](https://github.com/SillyTavern/SillyTavern) 移植为 Tauri v2 原生应用。前端完整保留上游体验（已同步上游 **1.19.0**，当前版本 2.4.0），后端从 Node/Express 重构为 **Rust Clean Architecture Cargo workspace**。数据格式与目录布局与 SillyTavern 完全兼容，许可 AGPL-3.0。

## Quick Start

前置要求：**Rust stable（edition 2024）· Node.js ≥ 22.13 · pnpm 11**（无其他系统依赖，Tauri CLI 由 pnpm 管理）

```bash
git clone https://github.com/Darkatse/TauriTavern.git
cd TauriTavern
pnpm install
pnpm run tauri:dev       # 桌面开发模式（自动构建前端 + 启动 Tauri）
```

常用命令（所有 tauri/android/ios 命令都经 `scripts/tauri-app.mjs --prepare-frontend` 包装，先构建前端再调 Tauri CLI）：

| 命令 | 作用 |
| --- | --- |
| `pnpm run check` | **完整 CI 门禁**（guardrails → 类型 → lint → 全部测试 → clippy），实现完成必跑 |
| `pnpm run web:build` | 仅构建前端资源包（Rspack） |
| `pnpm run tauri:build` | 构建桌面发行包；`tauri:build:portable` 便携版 |
| `pnpm run android:dev` / `ios:dev` | 移动端开发模式 |
| `pnpm run test:browser` / `test:contracts` / `test:ui` / `test:rust` | 分层测试（见「测试」） |
| `pnpm run fastools:run` | FasTools 开发调试工具箱 |
| `pnpm run tauri:dev:pilot` | 带 Tauri Pilot 的开发模式（AI Agent 界面调试） |

## 仓库布局

| 路径 | 内容 |
| --- | --- |
| `src/` | SillyTavern 上游前端（HTML/CSS/JS + jQuery/Bootstrap/Handlebars），禁止随意改动上游逻辑 |
| `src/tauri/main/` | **Tauri 注入层**（第一方代码）：`context/kernel/services/adapters/routes` 分层 |
| `src/scripts/extensions/` | 前端扩展；其中 `agent-system`、`in-app-agent`、`mcp-manager` 为 first-party React 严格 TS |
| `src-tauri/` | Cargo workspace 根（`Cargo.toml` 列出全部 18 个 crate） |
| `src-tauri/crates/tauritavern/` | Tauri host：shell、commands、composition root、platform glue（`tauri.conf.json` 也在这里） |
| `src-tauri/resources/` | tokenizer 等内置资源 |
| `docs/` | 架构与专题文档（编码前必读，见「文档索引」） |
| `scripts/` | guard 脚本、构建包装、CI、平台脚本 |
| `tests/` | 前端契约测试（node --test）+ `tests/browser/` 浏览器级测试 |
| `default/` | 首次启动的内置数据（角色卡/技能/脚手架，文件头注明 DO-NOT-EDIT） |
| `packages/app-use` | 共享 React hooks 包（workspace 依赖 `@tauritavern/app-use`） |
| `packaging/` `distribution/` | Flatpak / Linux 仓库 / Nix 缓存等分发资产 |
| `fastools/` | Rust 写的开发调试工具箱 |

## 后端架构（Clean Architecture，硬性约束）

crate 依赖只能外层→内层（由 `scripts/check-rust-crate-boundaries.mjs` 强制）：

- **`tt-domain`**：领域模型、值对象、领域错误、纯规则
- **`tt-contracts`**：跨 crate DTO、事件、payload、host resource 契约（前后端一致性来源）
- **`tt-ports`**：repository / gateway trait（接口放这里）
- **`tt-application`**：use case、service、编排
- **`tt-adapter-*`**（archive/extension/http/mcp/media/provider-http/storage-core/storage-userdata/sync/tokenization/triviumdb/vector/workspace-shell）：具体实现
- **`tauritavern`**（host）：只承载 Tauri shell、composition、commands、platform glue；Tauri-free 的 IO 必须进 adapter

## 前后端通信链路

```text
SillyTavern 前端 / 扩展
  → same-origin fetch / jQuery.ajax
  → src/tauri/main/interceptors.js → routes/*
  → context.safeInvoke(...)                    // window.__TAURITAVERN__
  → #[tauri::command]（仅存在于 presentation 层）
  → tt-application service → tt-ports trait → tt-adapter-* 实现
```

## 关键约定（详见 AGENTS.md「道/术」与 CONTRIBUTING.md）

- **文档优先**：编码前必读 `docs/BackendStructure.md`（含代码落点决策表 §9、新增后端能力最小流程 §10）、`docs/FrontendGuide.md`（新增注入接口流程 §9）、`docs/FrontendHostContract.md`。
- **项目哲学七原则**（取舍时遵循）：通用大于特判 · 第一性原理 · fail-fast 不静默降级 · 可维护性优先 · KISS 与 60/95 · 简洁与优雅 · 遵循但不拘泥。
- **分层纪律**：`#[tauri::command]` 只在 presentation 层且只调 application 服务；DTO 必须定义在 `tt-contracts` 并保持前后端一致；错误边界用 `CommandError`，领域/仓储用 `DomainError`；I/O 必须 tokio async。
- **兼容契约**：`/api/*` 只承载上游兼容行为；TauriTavern 新能力走 `window.__TAURITAVERN__.api.*`；Rust command 名不是扩展作者稳定 API。
- **数据兼容**：保持 `default-user`、`characters`、`chats`、`group chats`、`User Avatars`、`OpenAI Settings` 等目录/大小写语义与 JSONL、PNG card metadata 等格式。存储以文件布局为主（**非 SQLite**），私有状态放 `_tauritavern/`。
- **测试准入**：只为可观察的稳定行为、非平凡不变式、已修复的真实缺陷写测试；禁止正则断言源码、不测字面量与实现细节。
- **收尾标准**：逐路径自审数据链路与职责归属；测试通过只是行为核对，不构成收尾依据。
- **PR 目标分支 `dev`**（紧急修复才直接进 `main`）。

## 工程守护与测试

`pnpm run check` = 平台身份 → 前端 guardrails → TypeScript → eslint → logging/rust 边界 → 测试 → clippy：

- `check-frontend-guardrails.mjs` / `check-first-party-ui-guardrails.mjs`：守住前端注入层与 React+strict TS 边界（可 `--update-baseline`）
- `check-rust-crate-boundaries.mjs`：Rust 依赖方向；`check-logging-boundaries.mjs`：logging target 边界
- 测试四层：`tests/*.test.mjs` 契约测试（node --test）→ `tests/browser/`（需先 `web:build`）→ `rstest`（happy-dom，React UI）→ `cargo test --no-default-features --tests`

## 调试与平台

- **Tauri Pilot**：`pnpm run tauri:dev:pilot` 后 `tauri-pilot ping/snapshot/click/diff/logs`，交互前先 snapshot、每步一操作。
- 平台细节：`docs/AndroidDevelopment.md`、`docs/iOSDevelopment.md`、`docs/OpenHarmonyDevelopment.md`；便携模式 `TAURITAVERN_RUNTIME_MODE=portable` 或 `portable.flag`。

## 文档索引

| 主题 | 文档 |
| --- | --- |
| 技术栈总览 | `docs/TechStack.md` |
| 后端结构/crate 边界 | `docs/BackendStructure.md` |
| 前端集成/插件系统 | `docs/FrontendGuide.md` |
| Host ABI/请求拦截/资源契约 | `docs/FrontendHostContract.md` |
| 已落地模块状态 | `docs/CurrentState/README.md` |
| 扩展 API | `docs/API/README.md` |
| Agent 框架 | `docs/Agent/README.md` |

仓库级 Skill（`.agents/skills/` 为唯一来源，`.claude/skills` 是其软链接）：涉及架构规划、统一机制、上游同步或审阅时，使用 `maintain-conceptual-integrity`。
