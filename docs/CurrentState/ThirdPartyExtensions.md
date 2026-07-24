# 第三方前端扩展兼容现状

本文档描述 **当前已经落地** 的第三方前端扩展兼容实现，用于指导后续持续开发。

补充：目前已落地浏览器资源契约：

- 头像：`/thumbnail`、`/characters/*`、`/User Avatars/*`，并移除缩略图 DOM monkey patch
- 用户静态资源：`/backgrounds/*`、`/assets/*`、`/user/images/*`、`/user/files/*`

## 1. 范围与结论

当前兼容目标是 SillyTavern 风格的 **纯前端 third-party extension**，即依赖：

- `/scripts/extensions/third-party/<ext>/<path>` 这一同源静态资源前缀
- 浏览器原生子资源加载语义

当前方案的核心结论是：

> 兼容性的关键不是继续在前端 runtime 里“解释扩展代码”，而是把 third-party 资源路径重新做成 WebView 可原生加载的真实端点。

## 2. 当前链路

### 2.1 发现与激活

1. `src/scripts/extensions.js` 中的 `startOfflineExtensionsDiscovery()` 在 Host Ready 后请求 `/api/extensions/discover`
2. `src/tauri/main/routes/extensions-routes.js` 将请求转给 Rust 命令 `get_extensions`
3. `ExtensionService -> tt-adapter-extension::FileExtensionRepository::discover_extensions()`
4. 返回扩展列表后，前端继续读取 manifest 并缓存激活计划
5. 启动期先执行 `activateStartupSystemExtensions()`，只激活系统扩展
6. 若存在启用中的 local/global third-party 扩展，则在 `APP_READY` 后执行 `activateDeferredThirdPartyExtensions()`

补充约束：

- Rust 侧扩展仓储当前只读取 manifest 摘要元数据（如 `display_name` / `version` / `author` / `description` / `loading_order`）。
- `js` / `css` / `i18n` 等浏览器运行时字段不再由后端建模解释，而是由前端从原始 `manifest.json` 直接消费。

扩展命名约定：

- 系统扩展：`regex`、`quick-reply` 等
- 第三方扩展：统一命名为 `third-party/<folder>`

当前启动时序约束：

- third-party 扩展 discovery 可以提前，但 local/global third-party 模块求值不再阻塞 `APP_READY`
- `APP_READY` 发出后，晚加载的 third-party 扩展仍可依赖 `eventSource` 对 `APP_READY` 的 auto-fire 语义完成 ready 钩子
- `EXTENSION_SETTINGS_LOADED` 在存在待激活 third-party 扩展时会延后到 deferred activation 完成后再发出

### 2.2 安装、版本、分支与更新

第三方扩展管理已使用 Rust 原生 gitoxide 纵切面：

- 新安装只接受匿名 `http(s)` Git remote；不限制 GitHub、GitLab、Gitee，也不接受 provider Web tree URL、query/fragment、URL userinfo、SSH 或 credential helper。
- 未指定 ref 时使用 remote 的 born symbolic `HEAD`；显式 ref 按 branch-first、tag-second 解析。任意 revision expression/OID 不属于当前输入契约。
- 新安装直接生成标准 non-bare embedded `.git/`，不再生成 `_tauritavern/extension-sources` JSON。branch 使用 symbolic HEAD + upstream；tag 使用 detached peeled HEAD + exact tag refspec。
- version 只做 Git ref advertisement并比较 peeled remote OID 与 deployed HEAD，不 fetch、不写本地、不调用 provider REST API。
- branches 只通过 `ls-refs refs/heads/` 返回排序后的唯一远端 branch 短名、7 位 OID、current 与空 label；不 unshallow、不下载 branch object、不修改本地状态。
- switch 只接受远端 branch（兼容 `main` 与 `origin/main`），执行 exact depth-1 fetch、candidate preflight 和受管 worktree重建，最后提交标准 symbolic HEAD/upstream；切换当前 branch 是无网络 no-op，不同 branch指向当前 deployed OID时只切换refs/config，且不暗含 update hook。
- update 对 embedded repo 只做一次 exact depth-1 fetch；OID 相同不触碰 payload，OID 不同则先验证完整 candidate tree，再重建 payload/index，最后推进 branch ref 或 detached HEAD。
- legacy JSON extension 的 version/branches 只读 Git advertisement；central source JSON优先，早期扩展根目录内的 inline V1/V2 JSON作为只读 fallback，不在启动或读取时搬迁。首次 update（即使 OID 相同）或切换到不同 branch 时，在 sibling `.tmp-*` 中建立标准 embedded repo，验证完成后替换活动 snapshot并删除或自然淘汰 JSON。失败保留旧 snapshot与来源状态。
- provider REST、archive ZIP 与 host allowlist 后端已删除；所有 Git 错误均 fail-fast，不存在 REST/ZIP runtime fallback。

candidate preflight 会在删除活动 payload 之前验证完整 tree：portable UTF-8 path、大小写/NFC collision、file/directory shape、ODB object header kind、根 `manifest.json`内容。checkout 不执行 external filter/LFS/submodule；symlink 以普通文件物化，gitlink 只生成空目录占位。

install/update/switch/delete/move 与 LAN/TT Sync 的本地写操作共享一个 application-layer fail-fast permit；busy 映射为 409。discovery/version/branches 是只读操作，不占 permit。update 前端不会再用 AbortSignal 假取消已经进入 Rust 的磁盘写操作。

### 2.3 前端资源加载

资源 URL 由 `src/scripts/extensions/runtime/resource-paths.js` 统一生成：

- `getExtensionResourceUrl(name, path)`
- 对 third-party 扩展，最终 URL 为 `/scripts/extensions/third-party/<folder>/<path>`

激活时：

- JS 入口由 `asset-loader.js` 直接作为 `<script type="module" src="...">` 注入
- CSS 默认直接 `<link rel="stylesheet" href="...">` 加载
- 只有旧 WebView 不支持 CSS `@layer` 时，`third-party-runtime.js` 才会为样式 URL 附加 `ttCompat=layer`，并由 Rust 端点返回展平后的 CSS bytes
- `js` / `css` 字段显式接受 `string` 或单元素 `string[]`；不为多元素数组建立新的加载顺序语义

额外兼容层：

- `src/lib.js` 会把部分上游常用库挂到 `window`；其中 `window._`（lodash）是正式兼容 ABI，因为 JS-Slash-Runner、ST-Prompt-Template、MagVarUpdate 等生态扩展会在模块求值阶段直接访问 `_`
- `src/tauri/main/compat/mobile/mobile-runtime-compat.js` 负责旧 WebView 缺失 JS API 的 polyfills（仅 Tauri mobile）
- 第三方浮层/窗口 mobile surface compat（仅 Tauri mobile）：
  - 分类/契约输出：`src/tauri/main/compat/mobile/mobile-overlay-surface-admission.js`
  - 观察与有界 settle window：`src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js`
  - 同源 iframe contract bridge：`src/tauri/main/compat/mobile/mobile-iframe-viewport-contract-bridge.js`
- `src/scripts/browser-fixes.js` 保持与上游同步（不再承载 Tauri mobile compat）

### 2.4 后端资源提供

生产/打包运行时：

- `src-tauri/crates/tauritavern/src/lib.rs` 在主窗口安装 `on_web_resource_request`
- `src-tauri/crates/tt-application/src/services/host_resource_service/user_css.rs` 处理 `/css/user.css`
- `src-tauri/crates/tt-application/src/services/host_resource_service/third_party.rs` 处理 `/scripts/extensions/third-party/*`
- `src-tauri/crates/tt-application/src/services/host_resource_service/thumbnail.rs` 处理 `/thumbnail`
- `src-tauri/crates/tt-application/src/services/host_resource_service/user_data.rs` 处理用户数据静态资源：`/characters/*`、`/User Avatars/*`、`/backgrounds/*`、`/assets/*`、`/user/images/*`、`/user/files/*`

请求处理步骤：

1. 校验请求方法，只接受 `GET` / `HEAD` / `OPTIONS`
2. 通过 `src-tauri/crates/tt-contracts/src/client_asset_paths.rs` 解析并校验路径，再由 `src-tauri/crates/tt-application/src/services/host_resource_service/route_classifier.rs` 分类到具体资源处理器
3. 通过 `src-tauri/crates/tt-adapter-media/src/host_resources.rs` 一次完成 local/global 选源、打开文件和 metadata/revision 构造
4. 返回真实 bytes、正确 `Content-Type`、`Cache-Control: private, no-cache`、weak ETag 和 Last-Modified；条件命中时按 transport capability 返回 304 或完整 200（见 `docs/CurrentState/HostResourceCaching.md`）
   - 对用户静态资源端点（如 `/backgrounds/*`）若请求携带 `Range`，支持单范围并返回 `206 + Content-Range`（见 `docs/CurrentState/MediaAssetContract.md`）
5. 未命中时返回真正 `404`，不回退到 `index.html`

Host Resource 只校验浏览器 URL 的路径段，不禁止 data root 内部 symlink 指向外部目录或文件。这样用户可以让 SillyTavern 与 TauriTavern 共享同一套数据目录布局。

`.git` 是有意允许的路径组件，不是 Host Resource 或 TT-Sync 的名称级禁区。SillyTavern 迁移来的扩展可以携带标准 embedded `.git/`；显式请求 `/scripts/extensions/third-party/<folder>/.git/config`、`.git/HEAD` 等文件时走与其他扩展文件相同的 local-first、文件级读取与缓存流程，仍不提供目录浏览。TT-Sync 的 `extensions.local` / `extensions.third_party` 数据集也不会按 `.git` 名称排除这些路径。managed extension 根 `.git` 在仓库层是标准 Git 管理目录；这不改变路径层继续拒绝 `.`、`..`、编码分隔符、控制字符和路径逃逸。

这项兼容语义不代表 remote URL 可以保存秘密。当前第三方扩展本来就是同源可执行代码；任何 embedded `.git/config` 都必须只持久化脱敏 URL，私有仓库认证若未来支持，应使用设备本地 secret store。

开发态本地 Web 入口：

- `scripts/tauri-dev-server.mjs` 会在入口文档最前面注入 `src/dev-sw-bootstrap.js`；新 WebView 会话若继承旧 Service Worker controller，会先注销旧注册并重载一次，避免跨宿主进程复用失效的 Wry 协议过滤器
- `src/init.js` 会注册 `/tt-ext-sw.js`
- Service Worker 将 `/css/user.css`、`/scripts/extensions/third-party/*`、`/thumbnail`、`/characters/*`、`/User Avatars/*`、`/backgrounds/*`、`/assets/*`、`/user/images/*`、`/user/files/*` 转发到 `tt-ext` 自定义 scheme
- Rust 侧 `register_uri_scheme_protocol("tt-ext", ...)` 在 dev 下统一分发上述资源请求
- `convertFileSrc('', 'tt-ext')` 的结果可能因平台/WebView 不同而表现为 `tt-ext://localhost/` 或 `http(s)://tt-ext.localhost/`
- Service Worker 的直接 `tt-ext` fetch 保留原请求 cache mode；`Range`、`If-Range`、`If-None-Match`、`If-Modified-Since` 在直连或 IPC 路径中保持同一 Host Resource 语义，并通过 `Access-Control-Expose-Headers: *` 保留直连响应的完整表示头
- `If-Range`、`If-None-Match`、`If-Modified-Since` 会触发 custom scheme 跨源 preflight，必须直接通过通用 header wire 进入 IPC；其余 Service Worker `fetch(tt-ext)` 失败时切换到 window-context `fetch(tt-ext)`，正文以 transferable `ArrayBuffer` 返回。普通二进制资源不得经过 serde JSON IPC
- 非 Android 的 production、`tt-ext` 与 IPC fallback 都能代理 304；Android 按 Wry 硬约束返回完整 200。IPC 不另建 Cache API，因此自动生成 validator 的存储行为仍由具体 transport 决定

因此，开发态与生产态虽然入口不同，但 third-party 路径语义保持一致。

## 3. 数据目录与优先级

当前目录布局是：

- local third-party 扩展：`data/default-user/extensions/<folder>`
- global third-party 扩展：`data/extensions/third-party/<folder>`
- legacy 扩展来源元数据：优先读取 `data/_tauritavern/extension-sources/{local|global}/`；兼容更早版本的 `<extension>/.tauritavern-source.json`只读输入（新 embedded install 不再写入）

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
- `/css/user.css` 作为用户 CSS 覆盖文件
- `/thumbnail`、`/characters/*`、`/User Avatars/*` 作为头像相关的浏览器原生子资源端点
- `/backgrounds/*`、`/assets/*`、`/user/images/*`、`/user/files/*` 作为用户静态资源的浏览器原生子资源端点

当前安全约束：

- 拒绝缺失扩展目录、`.`、`..`
- 拒绝编码后的路径分隔符等非法路径
- 相对资源路径中的冗余 `/` 仅做等价归一化，不扩大访问范围
- `.git`、`.Git` 等名称按普通路径段接受；不要与 `.` / `..` 混同
- 只允许 third-party 前缀内的文件级读取，不提供目录浏览

## 5. 当前明确不支持或不承诺的内容

- 不支持 SillyTavern 的 Node-only backend plugins
- 不提供通用“前端伪静态服务器”或任意文件读取能力
- 不支持 private auth、SSH、credential helper、Git hook/external filter/LFS/submodule执行或递归 clone
- 不支持 gitfile/linked worktree、外部 common dir、外部 `core.worktree` 等非本地 Git 布局；管理操作会 fail-fast 并要求重新安装，不自动修复或读取 stale JSON fallback
- branch UI 不提供 local-only branch、tag/OID/revision switch、commit subject label、unshallow或完整历史
- 根 `.git/` 和 legacy source JSON 均不存在的扩展仍可被发现和加载，但 `update` 会要求重新安装
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
  - `src-tauri/crates/tt-contracts/src/client_asset_paths.rs`
  - `src-tauri/crates/tt-application/src/services/host_resource_service/route_classifier.rs`
  - `src-tauri/crates/tt-application/src/services/host_resource_service/third_party.rs`
  - 相关测试
- 若改动开发态代理链路，也必须同步验证 `src/dev-sw-bootstrap.js`、`src/init.js` 与 `src/tt-ext-sw.js`

## 7. 建议的最小回归面

每次调整后，至少回归以下几类能力：

- third-party 扩展可发现、可启用
- `manifest.json`、JS、CSS、图片/字体资源都能正确加载
- 显式 `.git/HEAD` 或无秘密 fixture 的 `.git/config` 与普通文件采用相同路径语义
- 不存在的资源返回 404，而不是 HTML fallback
- local/global 同名时仍保持 local 优先
- 旧 WebView 下的 CSS `@layer` 降级没有回归

如果一个问题已经超出以上边界，应先判断它是否属于“third-party 前端扩展兼容”范畴，再决定是否继续在这条链路上处理。
