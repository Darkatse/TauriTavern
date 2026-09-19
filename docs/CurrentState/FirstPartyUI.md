# First-party UI 当前架构

本文说明 TauriTavern 自有前端的现行架构与维护边界。SillyTavern 1.18.0 仍拥有页面、扩展加载和全局生命周期；TauriTavern 只在既有页面中挂载独立的 React client island，不建立第二套页面壳或路由。

## 1. 范围

First-party UI 包含：

- Agent System；
- MCP Manager；
- Tauri Settings、Dev Logs 与 Sync。

这些界面统一使用 React、React DOM 与 strict TypeScript/TSX。上游 SillyTavern 前端、第三方扩展和由 WebView 直接加载的宿主脚本不属于 React 组件源码。

## 2. 所有权与数据流

```text
Rust / Host services
  -> feature composition root
  -> typed controller / model / external store（按需）
  -> React client island
  -> island 自己的 DOM subtree
```

- Rust 与 Host service 拥有持久数据、跨窗口事实、文件和网络 I/O、Tauri command 及平台事件。
- Feature composition root 获取并校验 Host 能力，组装 DTO、typed actions、订阅、翻译器和挂载参数。
- Controller、model 或 external store 只在功能确实需要时承载异步编排和视图投影；简单界面直接使用局部 React state。
- React 组件负责渲染、未保存草稿和当前视图状态，不复制持久事实，也不直接调用 Tauri `invoke()`。
- 页面级事件必须使用 `window.SillyTavern.getContext()` 提供的 `eventSource` 与 `eventTypes`；订阅和退订使用同一个页面实例。
- 缺少必需 Host API、事件或 DTO 字段时立即失败并暴露上下文，不猜测默认能力。

## 3. 源码与构建边界

Rspack 直接拥有以下 TypeScript/TSX 源码：

```text
src/scripts/extensions/agent-system/src
src/scripts/extensions/mcp-manager/src
src/scripts/tauri/setting/settings-app
src/scripts/tauri/setting/dev-logs-app
src/scripts/tauri/setting/sync-app
```

入口和稳定输出如下：

| 功能 | Rspack entry | 输出 | 加载方 |
| --- | --- | --- | --- |
| Agent System | `agent-system/src/index.tsx` | `agent-system/dist/index.bundle.js` | Agent manifest |
| MCP Manager | `mcp-manager/src/index.tsx` | `mcp-manager/dist/index.bundle.js` | MCP manifest |
| Settings | `settings-app/SettingsApp.tsx` | `setting/dist/settings.bundle.js` | `setting-panel/settings-popup.js` |
| Dev Logs | `dev-logs-app/DevLogsApp.tsx` | `setting/dist/dev-logs.bundle.js` | `setting/dev-logs.js` |
| Sync | `sync-app/index.ts` | `setting/dist/sync.bundle.js` | Sync popup 与 listener 外壳 |

本地 TS/TSX 模块使用无扩展名相对 import。显式 `.js` import 只指向磁盘上真实存在、由浏览器执行的 JavaScript 模块。

`src/scripts/tauri/setting/*.js` 与 `setting-panel/*.js` 是 WebView 直接加载的宿主适配层。它们拥有 Popup 生命周期、保存/关闭时序、原生事件接线和 bundle 动态导入，因此继续以浏览器 ESM 交付。`format-bytes.js` 同时服务宿主脚本与 TS bundle，并在实现中通过 `// @ts-check` 和 JSDoc 提供类型信息。

Production 与 development 共用 `rspack.config.js` 的 `createRspackConfigs(mode)`。各 bundle 都是自包含 ES module，不依赖页面级 React 全局变量；React Compiler 只用于 Agent System。

## 4. 功能内部结构

### Agent System

- `index.tsx` 是扩展启动与主入口 composition root。
- `host-api.ts` 集中访问 `window.__TAURITAVERN__` 和 SillyTavern context。
- Panel、Timeline 与 Skill Manager 各自拥有 feature-local contract、controller/model 和 React presentation。
- `i18n.ts` 负责翻译与插值，消息 key 由静态 catalog 推导类型。
- `chat-input-toggle.ts` 与 `embedded-assets-buttons.ts` 是接入上游 DOM 的原生适配器，不创建额外 React root。

### MCP Manager

- `index.tsx` 负责挂载；`host.ts` 负责 Host 与 SillyTavern 边界。
- Manager、server dialog 与 test-call dialog 保持 feature-local，不依赖 Agent 或 Settings 的 UI runtime。

### Settings、Dev Logs 与 Sync

- Raw Settings 外壳创建 Popup、读取 Host 数据并向 React root 注入 typed options、actions 与 translator。
- React root 不读取外壳内部状态；外壳也不依赖 React component instance，只持有公开 mount handle。
- 扩展菜单快捷入口是设备本地的展示偏好；Raw Settings 外壳复用上游 Popup 管理选择，并由 DOM adapter 投影到既有 `#extensionsMenu`，不新增 React root 或后端设置。
- 三个功能共享同一 Rspack compiler，但保持独立 bundle 和独立生命周期。

## 5. 宿主调用契约

Settings 系列 bundle 通过 named export 暴露挂载函数：

| 挂载函数 | 返回 handle |
| --- | --- |
| `mountTauriTavernSettingsApp()` | `getDraft()`、`setChatBackupStorageStats()`、`unmount()` |
| `mountTauriTavernDevLogsApp()` | `unmount()` |
| `mountTauriTavernSyncApp()` | `refresh()`、`refreshAutomationStatus()`、`unmount()` |
| `mountTauriTavernSyncProgressApp()` | `update()`、`unmount()` |
| `mountTauriTavernSyncScopeApp()` | `getSelection()`、`unmount()` |

挂载参数在 JS/TS 边界校验。外壳只通过这些 handle 与 island 交互；新增能力应优先扩展对应 feature contract，而不是读取 React DOM 或组件内部状态。

Agent 与 MCP 由 manifest 加载后自行启动。它们的外部契约是 manifest 路径、Host ABI、事件/DTO 语义以及已挂载 DOM 的可观察行为。

## 6. 生命周期、DOM 与样式

- 每个 React root 只管理自己的 mount subtree；SillyTavern 继续管理外围文档和扩展容器。
- Host 订阅由 composition root、controller 或专用 hook 持有，并在对应生命周期结束时释放。
- 异步读取使用明确的 epoch、snapshot 或取消语义阻止旧结果覆盖新状态；不使用定时等待模拟协调。
- Popup 的 Save、Cancel、Escape、`onClosing` 和原生 dialog 行为由创建它的外壳或 composition root 拥有。
- 样式沿用 SillyTavern class、SmartTheme CSS 变量和移动端 surface 契约；React 不引入 CSS-in-JS 或独立 UI framework。
- 需要插入上游既有控件栏的简单按钮使用原生 DOM adapter；完整状态型界面才使用 React root。

## 7. 测试与开发

Rstest、Testing Library 与 `happy-dom` 覆盖组件交互、controller 行为、订阅生命周期和稳定边界。Node contract tests 覆盖 Host ABI 与浏览器 JavaScript 集成；Rust tests 覆盖后端领域和适配器行为。

`happy-dom` 不提供真实布局、滚动和平台 WebView 行为。涉及 pointer、focus、native dialog、scroll anchor、移动端 viewport 或主题级联的改动，还需要在真实 Tauri WebView 中验证。

常用命令：

```bash
pnpm run check:types
pnpm run check:lint
pnpm run test:ui
pnpm run web:build
pnpm run check
```
