# First-party UI 工程基线

本文记录 TauriTavern 自有 UI 完成 Vue 表示层退役后已经生效的工程契约。它不改变 SillyTavern 1.18.0 前端，也不把页面改造成 SPA。

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
- Agent、MCP、Settings 三个 first-party compiler 都直接编译 TS/TSX；自有 UI 已不再消费 Vue，遗留 dependency 与 Agent DefinePlugin 留给 Phase 6 独立删除。
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
| runtime `template:` | 0 |
| Vue imports | 0 |
| `createApp()` roots | 0 |

三个计数已经清零；任何回归都会直接失败，新增 `.vue` 文件同样失败。ESLint 同时保证：

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

Phase 2 后（2026-08-22）：Sync 为 218,632 raw / 67,633 gzip -9 bytes。Sync Main 迁移完成后 Vue runtime 退出 `sync.bundle.js`，Phase 1 的双 runtime 过渡成本（404,416 / 131,048）已消除，Sync entry 现在是纯 React。

Phase 3 后（2026-08-23）：Settings 为 209,392 raw / 64,869 gzip -9 bytes，Vue runtime 退出 `settings.bundle.js`，gzip 反而低于 Vue 基线约 4 KB。Dev Logs 仍携带 Vue runtime；是否改为共享 React chunk，必须等它迁移后用真实 stats 决定。

Phase 4 后（2026-08-23）：Dev Logs 为 201,756 raw / 63,449 gzip -9 bytes，Vue runtime 退出 `dev-logs.bundle.js`，gzip 比 Vue 基线低约 4 KB；Settings compiler 不再携带 Vue DefinePlugin。三个 React entry（Settings 209,392 / 64,869、Sync 218,632 / 67,633、Dev Logs 201,756 / 63,449）合计 629,780 raw / 195,951 gzip，各自重复打包 React runtime；是否引入共享 chunk 由真实 stats 与面板并存时的内存实测另行决定，本阶段不引入 `dependOn`/`splitChunks`。

Phase 4 UX 与状态一致性复审后（同日）：Live 面板 More 按钮并入状态行（`Showing x/y · +n new · Paused` + 按需出现的 Show older），行内时间戳缩写为本地化短时间（导出仍带完整日期时间），两个面板补空状态，LLM 面板以同步当前选择的条目下拉取代逐条 Prev/Next 盲翻（保留 Prev/Next），meta 对失败请求与加载错误以 `--fullred` 高亮；Dev Logs 最终为 202,991 raw / 63,965 gzip -9 bytes。

Agent System Phase 5 五个切片完成后（2026-08-24）：Agent entry 为 465,458 raw / 130,154 gzip -9 bytes，production module graph 只包含 React/React DOM，不再包含 Vue runtime/compiler；迁移前最后一个双 runtime 基线为 656,384 / 194,316。active/history Timeline 与 SubAgent 共用 mount-local typed controller、event session/detail epoch 和 immutable cached snapshot，完整已读历史保留在内存，virtualizer 只限制 DOM。平台 WebView 的 scroll/pointer/native dialog smoke 仍是发布门禁，不由 happy-dom 结果替代。

## 6. 当前有意不做的事情

- 不启用 React Compiler。Rspack 基底已经具备后续能力，但先用 pilot 的 profiler 和 bundle 数据证明收益。
- 不预建跨 feature 的 `ui-runtime`、全局 root registry、通用 controller 或 subscription framework。各 root 只保留已经出现的局部机制；Dev Logs 的 `useAsyncSubscription()` 只服务两个具有相同异步 cleanup 语义的真实订阅者，不提升为全局抽象。
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

Phase 1 与 Phase 2 已完成：Sync feature 的三个 root（Main、Progress、Scope）全部迁移为 React（`sync-app/SyncApp.tsx`、`SyncProgressApp.tsx`、`SyncScopeApp.tsx`），`sync.bundle.js` 不再包含 Vue runtime。公开 handle 契约不变，行为经 `SyncMain.test.tsx` / `SyncPilot.test.tsx` 与真实桌面 WebView smoke 验证；Pairing 区域合并等为有意的 UI/UX 调整，不声称逐节点 DOM parity。

Phase 3 已完成：Settings root 迁移为 React（`settings-app/SettingsApp.tsx`，view-model/draft 规范化与最小 patch 仍归外壳 `setting-panel/settings-*.js` 所有），`settings.bundle.js` 不再包含 Vue runtime。`getDraft()` 由 mount-local controller 同步供给，不存在 React commit 竞态；未编辑 Save 不再产生 dynamic theme fallback patch（计划批准的唯一语义修复）。`SettingsApp.test.tsx` 只覆盖公开边界、非平凡状态语义、已修复缺陷与异步生命周期，不测试源码文本、静态 DOM 清单或明显条件渲染。迁移后另做一轮用户批准的 UX 微调（行 hint 桌面端跨整行宽、disclosure 摘要改为状态文本），并评估后否决了侧边栏分页方案。真实 WebView 走查（Save/Close/Escape veto、purge 确认、Data Root 不回滚、窄屏与主题）待真机反馈。下一阶段进入 Dev Logs。

Phase 4 已完成：Dev Logs root 迁移为 React（`dev-logs-app/DevLogsApp.tsx`，初始读取、Popup、clipboard 与 Text Viewer 仍归外壳 `dev-logs.js` 所有），`dev-logs.bundle.js` 不再包含 Vue runtime。不设 controller/store：Host 订阅是 push-only 且每个 Popup 只有一个消费者，两个 Panel 通过共享的 `useAsyncSubscription()` 管理订阅生命周期（effect-event handler + per-setup 订阅者身份，StrictMode 下恰好一个活动订阅；异步 teardown 失败写入 console）。同一变更修复了共享 stream bridge 的两个根因缺陷（start 失败残留 ghost handler、末次退订与新订阅紧邻时 disable 覆盖 enable），并把 LLM 选择从会漂移的 numeric index 改为稳定 `selectedId`、Preview/Raw 增加 epoch stale 防护、keep 改为两阶段提交（setKeep 失败零提交；reload 失败不回滚已持久化 keep；只合并 reload 期间真实收到的事件）。公开 mount、Host、Popup、Text Viewer、关键 class 与 raw `<details>` 契约保持不变；状态行、短时间、空状态、条目下拉和错误高亮是后续批准的有意 UX 调整，不声称逐节点 Vue parity。真实 WebView 走查（追尾/暂停/300-800 窗口、Popup 栈与 Escape、三个 slash command 入口、窄屏与主题）待真机反馈；随后进入 Agent System 集中迁移。

Agent System Phase 5 的五个 root family 已完成代码迁移：Settings Entry、Embedded Assets、Main/History/Retention、Skill Manager、active/history Timeline + SubAgent 均为 React + strict TS/TSX；原生 chat-input/embedded-asset 按钮继续作为上游 DOM adapter。Timeline 保留 240-event paging、server-side invocation filter、120ms projection debounce、完整内存历史、窗口化 DOM、scroll anchor/follow-tail、resize 与 touch gesture 语义；History 保持只读，retry 继续走 typed generation action。当前 first-party guardrail 为 `0 / 0 / 0`，下一步是平台 WebView 验收与 Phase 6 删除 Vue dependency/DefinePlugin，而不是新增兼容层。
