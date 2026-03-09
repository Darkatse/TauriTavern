# 第三方前端扩展兼容现状

本文档描述 **当前已经落地** 的第三方前端扩展兼容实现，用于指导后续持续开发。

## 1. 范围与结论

当前兼容目标是 SillyTavern 风格的 **纯前端 third-party extension**，即依赖：

- `/scripts/extensions/third-party/<ext>/<path>` 这一同源静态资源前缀
- 浏览器原生子资源加载语义

当前方案的核心结论是：

> 兼容性的关键不是继续在前端 runtime 里“解释扩展代码”，而是把 third-party 资源路径重新做成 WebView 可原生加载的真实端点。

## 2. 当前链路

### 2.1 发现与激活

1. `src/scripts/extensions.js` 中的 `loadExtensionSettings()` 等待 `waitForTauriMainReady()`
2. 前端请求 `/api/extensions/discover`
3. `src/tauri/main/routes/extensions-routes.js` 将请求转给 Rust 命令 `get_extensions`
4. `ExtensionService -> FileExtensionRepository::discover_extensions()`
5. 返回扩展列表后，前端继续读取 manifest、排序并激活扩展

补充约束：

- Rust 侧扩展仓储当前只读取 manifest 摘要元数据（如 `display_name` / `version` / `author` / `description` / `loading_order`）。
- `js` / `css` / `i18n` 等浏览器运行时字段不再由后端建模解释，而是由前端从原始 `manifest.json` 直接消费。

扩展命名约定：

- 系统扩展：`regex`、`quick-reply` 等
- 第三方扩展：统一命名为 `third-party/<folder>`

### 2.2 前端资源加载

资源 URL 由 `src/scripts/extensions/runtime/resource-paths.js` 统一生成：

- `getExtensionResourceUrl(name, path)`
- 对 third-party 扩展，最终 URL 为 `/scripts/extensions/third-party/<folder>/<path>`

激活时：

- JS 入口由 `asset-loader.js` 直接作为 `<script type="module" src="...">` 注入
- CSS 默认直接 `<link rel="stylesheet" href="...">` 加载
- 只有旧 WebView 不支持 CSS `@layer` 时，`third-party-runtime.js` 才会预取 CSS、展平 `@layer`、绝对化 `url()` / `@import`，再回落到 Blob URL
- `js` / `css` 字段显式接受 `string` 或单元素 `string[]`；不为多元素数组建立新的加载顺序语义

额外兼容层：

- `src/lib.js` 会把部分上游常用库挂到 `window`
- `src/tauri/main/compat/mobile/mobile-runtime-compat.js` 负责旧 WebView 缺失 JS API 的 polyfills（仅 Tauri mobile）
- `src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js` 负责第三方浮层 safe-area top 兜底（仅 Tauri mobile）
- `src/scripts/browser-fixes.js` 保持与上游同步（不再承载 Tauri mobile compat）

### 2.3 后端资源提供

生产/打包运行时：

- `src-tauri/src/lib.rs` 在主窗口安装 `on_web_resource_request`
- `src-tauri/src/presentation/web_resources/third_party_endpoint.rs` 仅拦截 `/scripts/extensions/third-party/*`

请求处理步骤：

1. 校验请求方法，只接受 `GET` / `HEAD` / `OPTIONS`
2. 通过 `src-tauri/src/infrastructure/third_party_paths.rs` 解析并校验路径
3. 通过 `src-tauri/src/infrastructure/third_party_assets.rs` 定位文件并推断 MIME
4. 返回真实 bytes、正确 `Content-Type`、`Cache-Control: no-store`
5. 未命中时返回真正 `404`，不回退到 `index.html`

开发态本地 Web 入口：

- `src/init.js` 会注册 `/tt-ext-sw.js`
- Service Worker 将 `/scripts/extensions/third-party/*` 转发到 `tt-ext` 自定义 scheme
- Rust 侧 `register_uri_scheme_protocol("tt-ext", ...)` 复用同一套 third-party 资源处理逻辑

因此，开发态与生产态虽然入口不同，但 third-party 路径语义保持一致。

## 3. 数据目录与优先级

当前目录布局是：

- local third-party 扩展：`data/default-user/extensions/<folder>`
- global third-party 扩展：`data/extensions/third-party/<folder>`
- 扩展来源元数据：`data/_tauritavern/extension-sources/{local|global}/`

当前优先级规则：

- 发现时：若 local 与 global 同名，保留 local，跳过 global
- 读资源时：先查 local，再查 global

这意味着 local 扩展可以覆盖同名 global 扩展。

## 4. 当前已支持的兼容边界

当前目标是恢复 SillyTavern third-party 扩展依赖的“静态资源契约”。因此下列路径应按浏览器默认语义工作：

- `<script type="module" src="...">`
- `<link rel="stylesheet" href="...">`
- ESM 相对导入
- `fetch('/scripts/extensions/third-party/...')`
- CSS `url(...)`
- iframe 页面及其相对资源

当前安全约束：

- 拒绝空路径段、`.`、`..`
- 拒绝编码后的路径分隔符等非法路径
- 只允许 third-party 前缀内的文件级读取，不提供目录浏览

## 5. 当前明确不支持或不承诺的内容

- 不支持 SillyTavern 的 Node-only backend plugins
- 不提供通用“前端伪静态服务器”或任意文件读取能力
- `branches` / `switch` 路由在 Tauri 后端仍未实现
- 没有来源元数据的扩展仍可被发现和加载，但不能可靠更新；`update` 会要求重新安装
- third-party runtime 不再负责通用 JS 源码重写，不应再把它扩展回“大而全解释器”

## 6. 持续开发约束

后续若继续改 third-party 兼容，先问三个问题：

1. 这是浏览器资源契约没有做对，还是某个平台运行时缺能力？
2. 这个问题应该修在前端加载编排层，还是应该修在后端资源端点？
3. 这个修复会不会重新把系统带回“前端模拟服务器”的方向？

推荐维护原则：

- 保持 `/scripts/extensions/third-party/*` 作为唯一资源契约，不轻易改路径
- 不要把 `/api/*` 请求拦截和 third-party 静态资源端点混回同一层
- 新兼容修复优先做成“最小能力补丁”，不要重新引入广泛源码扫描或 eager 预取
- 若调整路径规则，至少同步检查：
  - `src/scripts/extensions/runtime/resource-paths.js`
  - `src-tauri/src/infrastructure/third_party_paths.rs`
  - `src-tauri/src/presentation/web_resources/third_party_endpoint.rs`
  - 相关测试
- 若改动开发态代理链路，也必须同步验证 `src/init.js` 与 `src/tt-ext-sw.js`

## 7. 建议的最小回归面

每次调整后，至少回归以下几类能力：

- third-party 扩展可发现、可启用
- `manifest.json`、JS、CSS、图片/字体资源都能正确加载
- 不存在的资源返回 404，而不是 HTML fallback
- local/global 同名时仍保持 local 优先
- 旧 WebView 下的 CSS `@layer` 降级没有回归

如果一个问题已经超出以上边界，应先判断它是否属于“third-party 前端扩展兼容”范畴，再决定是否继续在这条链路上处理。
