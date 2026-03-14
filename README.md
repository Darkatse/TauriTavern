# TauriTavern

TauriTavern 将 SillyTavern 移植为基于 Tauri v2 + Rust 后端的原生应用，同时保留上游前端体验。前端已同步至 SillyTavern 1.16.0，并通过模块化的 Tauri 注入层进行集成。

[English](README_EN.md)

## 特性亮点

- 基于 Tauri v2 的原生桌面运行时，支持 Windows、macOS、Linux
- Rust 后端采用整洁架构分层设计
- 前端兼容 SillyTavern 1.16.0
- 支持多种 Chat Completion 提供商：OpenAI、Claude、Gemini（MakerSuite）以及自定义 OpenAI 兼容端点
- 模块化请求注入管线（`src/tauri/main/*`），并收敛为可维护的 Host Kernel 分层（`context/kernel/services/adapters/routes`）
- 平台 ABI：`window.__TAURITAVERN__`（小而稳定的宿主对外接口）+ 请求追踪 header：`x-tauritavern-trace-id`
- 工程守护：严格类型检查（`tsc -p tsconfig.host.json`）+ guardrails（依赖边界/行数预算/路由禁止引用 `window`）
- 统一的前端引导管线，无需运行时加载器间接层

## 架构概览

### 后端（`src-tauri`）

- `presentation`：Tauri 命令与 API 边界层
- `application`：用例/服务与 DTO 编排层
- `domain`：核心模型、契约、错误定义
- `infrastructure`：文件持久化、仓储实现、日志

### 前端（`src`）

- 上游 SillyTavern 前端代码（HTML/CSS/JS）
- Tauri 桥接与拦截层，将 HTTP 请求替换为本地 Tauri 命令调用

前端启动流程：

1. `src/init.js` 依次加载 `lib.js` → `tauri-main.js` → `script.js`
2. `src/lib.js` 静态导入 `src/dist/lib.bundle.js` 并导出稳定的 ESM 库接口
3. `src/tauri-main.js` 委托给 `bootstrapTauriMain()`
4. `src/tauri/main/bootstrap.js` 创建上下文/路由/拦截器，安装 `window.__TAURITAVERN__` 平台 ABI，并为宿主接管路由注入追踪 header

## 前端集成结构

```text
src/
├── tauri-bridge.js              # 底层 Tauri 桥接（invoke/listen/convertFileSrc）
├── tauri-main.js                # 轻量引导入口
├── init.js                      # 启动编排器
├── lib.js                       # 库门面（ESM 导出）
├── dist/lib.bundle.js           # webpack 构建的第三方依赖包
└── tauri/main/
    ├── bootstrap.js             # 组合根（composition root）
    ├── context.js               # 兼容 shim（re-export `context/index`）
    ├── context/                 # Host Kernel facade + types（对外契约保持稳定）
    ├── kernel/                  # 纯逻辑（策略/追踪/键生成/格式化等）
    ├── services/                # 有状态能力（assets/thumbnails/characters/android…）
    ├── adapters/                # 触碰 window/DOM/上游 ST 的适配层
    ├── http-utils.js            # 请求/响应解析工具
    ├── interceptors.js          # fetch/jQuery ajax 拦截补丁
    ├── router.js                # 轻量路由注册表
    └── routes/
        ├── system-routes.js
        ├── settings-routes.js
        ├── extensions-routes.js
        ├── resource-routes.js
        ├── character-routes.js
        ├── chat-routes.js
        └── ai-routes.js
```

## 开发指南

前置要求：

- Rust stable
- Node.js 18+
- pnpm
- Tauri CLI

环境搭建：

```bash
git clone https://github.com/Darkatse/tauritavern.git
cd tauritavern
pnpm install
```

常用命令：

```bash
pnpm run check             # guardrails + host kernel 类型检查（推荐每次改动后先跑）
pnpm run web:build         # 构建前端资源包（webpack）
pnpm run dev           # 桌面开发模式（等价 tauri:dev）
pnpm run tauri:dev     # 桌面开发模式
pnpm run tauri:build   # 构建桌面发行包
pnpm run android:dev   # Android 开发模式
pnpm run ios:dev       # iOS 开发模式
```

便携版构建补充说明：

- `pnpm run tauri:build:portable` 默认输出到 `release/portable/`
- 可通过 `TAURITAVERN_RUNTIME_MODE=portable` 或 `portable.flag` 显式启用便携运行策略
- Windows 便携版需用户自行确保 WebView2 运行时可用

## FasTools（调试工具）

`fastools` 是一个极其有用的小工具箱，方便开发与桌面端部署时的调试。

构建：

```bash
pnpm run fastools:build
```

运行：

- `pnpm run fastools:run`

如需直接使用 cargo，也可在仓库根目录执行：

```bash
cargo build --release --manifest-path fastools/Cargo.toml
cargo run --manifest-path fastools/Cargo.toml
```

## 项目文档

- `docs/FrontendGuide.md`：前端架构与扩展指南
- `docs/FrontendHostContract.md`：宿主层对外契约（重构时优先保障不回归）
- `docs/BackendStructure.md`：后端架构详解
- `docs/TechStack.md`：技术栈与集成选型
- `docs/ImplementationPlan.md`：路线图与里程碑
- `docs/CurrentState/README.md`：已落地模块的当前实现状态说明

## 许可协议

AGPL-3.0（与 SillyTavern 同系列许可协议）。

## 致谢

- [SillyTavern](https://github.com/SillyTavern/SillyTavern)
- [Tauri](https://tauri.app/)
- [Cocktail](https://github.com/Lianues/cocktail)
- [Tavern-Helper](https://github.com/N0VI028/JS-Slash-Runner)
- [LittleWhiteBox](https://github.com/RT15548/LittleWhiteBox)
- [MikTik](https://github.com/Darkatse/MikTik)
