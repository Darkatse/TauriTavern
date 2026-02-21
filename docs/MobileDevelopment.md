# TauriTavern 移动端开发说明

本文档记录当前移动端（Android / iOS）开发中已经踩过的关键问题、根因分析、已落地方案，以及对应的架构改动。目标是避免重复踩坑，并为后续替换官方修复留出清晰迁移路径。

## 1. Android WebView 安全区注入时机竞态

### 1.1 现象

- `#top-settings-holder` 偶发沉入状态栏。
- 现象不稳定：同一版本在不同启动时机下表现不同。
- 简单删除延时重试后，问题明显回归。

### 1.2 根因

根因不是 inset 数值计算本身，而是 **注入时机竞态**：

- Android WebView 启动阶段常经历 `about:blank -> tauri.localhost` 的页面切换。
- 若在 `about:blank` 或 `document.readyState=loading` 时注入 CSS 变量，后续导航会丢失变量。
- 表层看是“safe area 失效”，本质是“注入到了错误上下文或过早上下文”。

参考问题：  
https://github.com/tauri-apps/tauri/issues/14240

### 1.3 当前实现

核心入口仍是 `src-tauri/gen/android/app/src/main/java/com/tauritavern/client/MainActivity.kt`，但职责已拆分为：

- `AndroidInsetsBridge.kt`：系统栏/IME inset 监听与 CSS 变量注入；
- `WebViewReadinessPoller.kt`：页面就绪轮询；
- `ShareIntentParser.kt`：分享 Intent 解析与导入文件持久化；
- `SharePayloadDispatcher.kt`：分享 payload 队列与前端 bridge 分发；
- `MainActivity.kt`：仅保留生命周期编排与模块协作。

- 保留 edge-to-edge 与透明系统栏配置（沉浸基础）；
- 监听系统栏与 IME inset；
- 将 inset 写入前端 CSS 变量：
  - `--tt-safe-area-top/right/left/bottom`（系统安全区）
  - `--tt-ime-bottom`（输入法可见时的底部 inset）
  - `--tt-viewport-bottom-inset`（前端通过 `max()` 合成有效底部 inset）
- 注入前先检查页面就绪：
  - `document.readyState !== 'loading'`
  - `location.href !== 'about:blank'`
  - 未满足时进行有限次短重试。

前端消费变量在：

- `src/style.css`（变量定义与 fallback）
- `src/css/mobile-styles.css`（顶部栏与容器定位使用变量）

### 1.4 维护原则

- 不要把“就绪态判断”误删为一次性注入。
- 不要把此问题误判为纯 CSS 问题；先验证变量是否被注入到正确页面上下文。
- 若后续 Tauri 官方修复 WebView safe-area 注入时序，可再评估收敛逻辑。

---

## 2. Android 资源访问语义差异（APK assets）

### 2.1 官方语义

Tauri 官方说明：Android 资源位于 APK assets，不是普通文件系统路径，返回值可能为 `asset://localhost/...`，需要通过 fs 插件语义访问。  
https://v2.tauri.app/develop/resources/#android

### 2.2 过去的问题

- 模板文件读取失败（如 popup/template 相关异常）。
- 默认内容索引读取失败（`default/content/index.json` not found）。
- 直接按“普通路径”处理资源导致跨平台行为不一致。

### 2.3 架构改动（资源层收敛）

#### A. 构建期生成资源索引与嵌入映射

`src-tauri/build.rs` 现在会：

- 扫描 `../default/content` 和 `../src/scripts/templates`；
- 生成 `default_content_manifest.json`（默认内容清单）；
- 生成 `embedded_resources.rs`（虚拟路径 -> `include_bytes!` 映射）。

#### B. 运行时统一资源访问入口

`src-tauri/src/infrastructure/assets.rs` 提供统一 API：

- `read_resource_bytes`
- `read_resource_text`
- `read_resource_json`
- `copy_resource_to_file`
- `list_default_content_files_under`

平台策略：

- Android：优先走构建期嵌入资源映射；
- 非 Android：走 `BaseDirectory::Resource` + fs 访问。

#### C. 前后端模板读取解耦

- 后端新增命令：`read_frontend_template`  
  文件：`src-tauri/src/presentation/commands/bridge.rs`
- 前端模板加载改为 Tauri 环境下优先 invoke：  
  文件：`src/scripts/templates.js`

#### D. 默认内容初始化改为“资源 -> 真实文件”复制流程

`src-tauri/src/infrastructure/repositories/file_content_repository.rs` 不再依赖资源目录的直接文件路径语义，改用统一资源接口复制到用户目录。

---

## 3. iOS / Android 应用数据目录解析异常

### 3.1 问题背景

在移动端，Tauri 提供的目录 API 在不同平台/版本可能与预期目录不一致。  
已确认 Android 存在已知问题：`appDataDir/localDataDir` 可能返回内部路径（如 `/data/user/0/...`）而非外部 app 目录（如 `/storage/emulated/0/Android/data/...`）。

### 3.2 当前方案：单点路径解析抽象

新增单点路径解析模块：  
`src-tauri/src/infrastructure/paths.rs`

统一入口：

- `resolve_app_data_dir(app_handle)`

当前行为：

- Android：优先使用 `app_data_dir`，仅当其落在内部目录（如 `/data/user/0/...`）时，自动回退到从 `document_dir` 推导外部 app data 目录；
- 其他平台（含 iOS）：回退到标准 `app_data_dir`。

### 3.3 架构收益

- 所有仓储与应用数据根路径都通过同一函数解析；
- 平台差异被收敛到一个模块，不向业务层扩散；
- 未来若 iOS 出现类似目录异常，可在同一模块增加 `cfg(target_os = "ios")` 分支，不需要修改各仓储。

---

## 4. 与上述问题相关的关键架构调整

### 4.1 基础设施层

- 新增 `infrastructure::assets`（资源读取/复制统一抽象）
- 新增 `infrastructure::paths`（应用数据目录统一抽象）
- `infrastructure::mod.rs` 导出上述模块

### 4.2 应用初始化与数据根目录

- `src-tauri/src/app.rs` 的 `resolve_data_root` / `resolve_log_root` 已改为依赖 `resolve_app_data_dir`

### 4.3 资源协议访问权限

- `src-tauri/src/lib.rs` 在 setup 阶段对 `data_root` 执行：
  - `asset_protocol_scope().allow_directory(&data_root, true)`
- 目的：允许 WebView 通过 asset 协议访问用户数据文件，避免前端资源加载 403。

### 4.4 前端接入点

- `src/scripts/templates.js`：模板读取在 Tauri 环境下走 `invoke('read_frontend_template')`
- `src/css/mobile-styles.css` + `src/style.css`：通过 `--tt-safe-area-*` 消费原生注入的安全区变量

---

## 5. 后续迁移与清理建议

1. **Tauri 官方修复目录 API 后**  
   `infrastructure/paths.rs` 会自动优先使用修复后的 `app_data_dir`，无需在仓储层做分散修补。

2. **Tauri 官方修复 WebView safe-area 注入后**  
   可评估简化 `MainActivity` 的“页面就绪后注入”逻辑，但必须先验证不会回归 `about:blank` 时序竞态。

3. **新增移动端特性时**  
   优先复用现有单点抽象（`assets.rs` / `paths.rs` / `MainActivity.kt`），避免再次把平台差异扩散到业务代码。

---

## 6. 插件系统（前端）移动端兼容补丁

以下问题仅在 Android 旧 WebView 上高概率出现，桌面端通常不复现。

### 6.1 `*.at is not a function`

现象：

- 第三方插件初始化报错（典型如 `g.at is not a function`）。

根因：

- 插件构建产物使用了较新的 JS API（`Array/String.at`、`toSorted`、`findLastIndex` 等）。
- 旧 Android WebView 缺少这些 API。

已落地方案：

- 在 `src/scripts/browser-fixes.js` 增加移动端按需兼容层：
  - 仅在移动端且检测到缺失 API 时执行；
  - 仅执行一次；
  - 桌面端零开销。

### 6.2 插件面板样式大面积失效（如 `TH-custom-tailwind` 布局错乱）

现象：

- 插件 CSS 文件请求成功，但大量样式未生效，界面排布混乱。

根因：

- 旧 Android WebView 对 CSS Cascade Layers（`@layer`）支持不完整。
- 采用 Tailwind v4 打包的插件会把大量规则放在 `@layer` 中，导致整层失效。

已落地方案：

- 在 `src/scripts/extensions/runtime/third-party-runtime.js` 的样式加载链路中：
  - 先探测当前 WebView 是否支持 `@layer`；
  - 不支持时用 `css-tools` 将 `@layer` 规则展平后再注入。

性能策略：

- 支持 `@layer` 的环境走快路径，不转换；
- 能力检测结果缓存；
- 预处理结果走现有样式缓存，避免重复计算。


### 6.3 JS-Slash-Runner 脚本弹窗贴顶（关闭按钮落入状态栏）

现象：

- 某些脚本运行后弹窗顶部被状态栏遮挡，关闭按钮不可点击。

根因：

- 脚本运行时直接向主文档注入 `<style>`；
- 规则常见为 `position: fixed` + `top: 0`，绕过了扩展 CSS 资源链路中的现有修正。

已落地方案：

- 在 `src/scripts/browser-fixes.js` 增加移动端动态样式补丁：
  - 监听运行时新增 `<style>`，修正固定定位规则中的 `top`；
  - 同步监听节点新增与 `class/style` 变更，兜底修正 fixed 元素行内/计算后的 `top`；
  - 把未包含 safe-area 的 `top` 统一重写为 `max(var(--tt-safe-area-top), <原值>)`。

设计约束：

- 仅移动端生效；
- 仅作用于运行时动态 `<style>`，不改静态主样式文件；
- 不侵入第三方扩展资源加载链路（与 `third-party-runtime.js` 解耦）。
