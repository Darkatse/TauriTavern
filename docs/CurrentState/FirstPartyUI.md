# First-party UI 工程基线

本文记录 TauriTavern 自有 UI 在 Vue 退役期间已经生效的工程契约。它不改变 SillyTavern 1.18.0 前端，也不把页面改造成 SPA。

## 1. 所有权

```text
Rust / Host ABI（持久事实、平台能力、事件）
  -> feature composition root（DTO、actions、subscription、Popup/DOM 桥）
  -> React client island（局部草稿和视图状态）
  -> island 自己的 DOM subtree
```

- React 是 TauriTavern 自有状态型 UI 的统一表示层。
- SillyTavern 继续拥有文档、扩展加载和全局生命周期。
- Rust/Host 继续拥有持久事实和跨窗口事实；React 不建立第二份平台状态。
- 迁移以完整 root 为单位，不建立 Vue/React 组件桥。
- 上游前端及其依赖不纳入这次现代化范围。

当前 strict TSX 所有权范围只有三处：

```text
src/scripts/extensions/agent-system/src/**/*.{ts,tsx}
src/scripts/extensions/mcp-manager/src/**/*.{ts,tsx}
src/scripts/tauri/setting/**/*.{ts,tsx}
```

`tsconfig.ui.json`、ESLint 和 Rstest 显式使用这三个范围。尚未存在 TSX 的目录也保留在配置中，使后续原地迁移从第一个文件开始就进入门禁。

## 2. 构建与开发链路

`rspack.config.js` 的 `createRspackConfigs(mode)` 是 production 与 development 的唯一构建图：

- `pnpm run web:build` 显式使用 production mode、persistent cache 和 fail-fast build。
- `pnpm run web:dev` 显式使用 development mode、memory cache 和 watch。
- Agent、MCP、Settings 三个 first-party compiler 都能直接编译 TS/TSX；Vue compiler 只为尚未迁移的 root 保留。
- production cache 由 Rspack 自己跟踪模块依赖；四个 compiler 使用独立目录。源码内容不再被预先哈希进 cache version，单文件修改不会人为废弃整个缓存。

标准 `pnpm dev` 不再先生成 production bundle。`scripts/tauri-dev-server.mjs` 直接拥有同一份 development MultiCompiler：

```text
启动 -> development 初次编译成功 -> HTTP server 开始监听
TS/TSX 变化 -> Rspack 重编译 -> 成功后发送 reload
                              -> 失败则保留页面并报告错误
普通未打包资源变化 -----------> 直接发送 reload
```

因此，页面不会在编译失败时刷新到旧 bundle；进程退出时 watcher 和 compiler 都会关闭。现阶段继续使用既有 dev server，是因为它还负责 service worker bootstrap 和 Tauri reload URL；没有引入第二个 dev server 或 HMR 协议。

## 3. 自动门禁

`scripts/check-first-party-ui-guardrails.mjs` 被 `pnpm run check:frontend` 调用。当前 Vue 债务基线为：

| 指标 | 上限 |
| --- | ---: |
| runtime `template:` | 35 |
| Vue imports | 8 |
| `createApp()` roots | 10 |

这些值只能随完整 root 迁移而下降；新增 `.vue` 文件直接失败。ESLint 同时保证：

- 新的 typed first-party UI 不得 import Vue；
- presentation 文件默认不超过 500 行；
- MCP 的既有 `host.ts`（663 行）和 `test-call-dialog.tsx`（613 行）以当前值为上限，只能持平或缩短。

门禁是迁移棘轮，不是源码解析器。每迁移一个 root，都必须在同一变更中删除旧 Vue 实现并下调对应计数。

## 4. 冻结的宿主契约

第一轮迁移只替换 island 内部表示层。以下外壳 handle 与调用时序保持不变：

- Settings：`getDraft()`、`setChatBackupStorageStats()`、`unmount()`；
- Sync：`refresh()`、`refreshAutomationStatus()`、`unmount()`；
- Sync Progress：`update()`、`unmount()`；
- Sync Scope：`getSelection()`、`unmount()`；
- Dev Logs：`unmount()`。

Popup 的 Save/Cancel/onClosing、关键 class/ID、SmartTheme 变量、滚动容器、移动端 surface 和文案顺序同样属于外部可观察契约。迁移不顺带重写 Host API、CSS 或交互设计。

## 5. 迁移前 production bundle 基线

Rspack 2.1.10、React 19.2.8、Vue 3.5.41 下的 2026-08-20 基线：

| bundle | raw bytes | gzip -9 bytes | framework |
| --- | ---: | ---: | --- |
| Agent System | 501,628 | 134,165 | Vue runtime compiler |
| MCP Manager | 230,039 | 71,148 | React |
| Settings | 203,922 | 68,782 | Vue runtime compiler |
| Dev Logs | 192,141 | 67,412 | Vue runtime compiler |
| Sync | 217,240 | 72,093 | Vue runtime compiler |

Settings、Dev Logs 与 Sync 当前各自携带 Vue runtime。是否改为共享 React chunk，必须等这些入口迁移后用真实 stats 决定；不能为了预想中的几十 KB 引入全局 runtime ABI 或 Module Federation。

## 6. 当前有意不做的事情

- 不启用 React Compiler。Rspack 基底已经具备后续能力，但先用 pilot 的 profiler 和 bundle 数据证明收益。
- 不预建 `ui-runtime`、全局 root registry、通用 controller 或 async subscription hook。当前 MCP 常驻入口由扩展 loader 保证单次执行，临时 dialog 已显式 `finally -> unmount()`；Phase 1 出现第二个相同生命周期后再抽取共同机制。
- 不在 Phase 0 拆分 MCP 或修改聊天关键路径。行数棘轮先阻止债务增长，真实 feature 改动再沿职责边界拆分。
- 不引入 router、全局状态库、query cache、CSS-in-JS、UI framework、Zod 或新测试框架。

mount/listener 泄漏、Popup 视觉、focus/scroll/pointer 和移动端内存需要真实 React pilot 与 WebView 才能测量。它们是进入 Settings/Agent 大规模迁移前的 Go/No-Go 证据，不用静态脚手架伪造基线。

## 7. 验证入口

```bash
pnpm run check:frontend
pnpm run check:types
pnpm run check:lint
pnpm run test:ui
pnpm run web:build
pnpm run check
```

Phase 1 首选 Sync Progress 与 Sync Scope：它们分别验证 imperative `update()` 和同步 `getSelection()`，能够以最小风险证伪 mount handle、Popup cleanup、development watch 与移动端行为。
