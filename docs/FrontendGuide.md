# TauriTavern 前端指南

本文档描述 TauriTavern 当前前端（基于 SillyTavern 1.19.0）在 Tauri 环境下的集成架构与开发方式。

宿主层对外契约清单见：`docs/FrontendHostContract.md`（重构时优先保障其不回归）。

共同控件与无障碍约定见 [FrontendAccessibility](FrontendAccessibility.md)。

## 1. 目标与原则

- **最小侵入**：尽量保持上游 SillyTavern 前端行为不变。
- **模块化**：将 Tauri 注入逻辑拆分为独立模块，避免单文件膨胀。
- **低耦合**：路由注册、请求拦截、业务上下文分离。
- **入口收敛**：统一走 `init.js -> tauri-main.js -> tauri/main/*`，减少重复入口。

## 2. 启动链路

当前前端启动顺序如下（宿主身份在页面脚本运行前已写入，见 2.1）：

1. `src/init.js` 动态导入：`lib.js` -> `tauri-main.js` -> `script.js`
2. `src/lib.js` 静态导入 `src/dist/lib.core.bundle.js`，统一提供 ESM 导出；`highlight.js` 通过 `getHljs()` 按需加载
3. `src/tauri-main.js` 仅调用 `bootstrapTauriMain()`（薄入口）
4. `src/tauri/main/bootstrap.js` 负责：
   - 创建运行上下文（`context`）
   - 注册前端路由（`router + routes/*`）
   - 安装请求拦截器（`fetch` 与 `jQuery.ajax`）
   - 安装平台 ABI：`window.__TAURITAVERN__`（小而稳定的宿主对外接口）
   - 安装同源窗口下载桥（所有宿主的浏览器式导出 → 原生交付）
   - 安装 Tauri mobile 兼容层（runtime polyfills，仅移动端）
   - 为宿主接管的路由响应注入追踪 header：`x-tauritavern-trace-id`
   - 初始化 bridge 与目录信息
   - Tauri 就绪后安装窗口快照与窗口背景发布（仅移动端）

### 2.1 宿主身份

宿主身份 `{ platform, kind }` 由 Rust 按编译目标生成（契约见 `docs/FrontendHostContract.md` §2.2）。前端只通过 `src/scripts/util/host-identity.js` 读取：`hostPlatform()` 返回平台，`isMobileHost()` / `isDesktopHost()` 返回宿主类别。测试用 `tests/helpers/host-identity.mjs` 安装身份。

与平台有关的判断，先分清在问什么：

| 问题 | 依据 | 例子 |
| --- | --- | --- |
| 宿主属于哪一类外壳 | `isMobileHost()` / `isDesktopHost()` | 移动端兼容层、F11 全屏、数据目录选择、软键盘相关的自动聚焦 |
| 选用哪个平台的实现，或某个平台独有的问题 | `hostPlatform()` | Windows 托盘、Android WebView 的剪贴板与 IME |
| 可选能力是否存在 | 插件、原生桥或 DOM API 本身 | 扫码插件、Android 原生桥 |
| 屏幕与交互形态 | 上游 Bowser `isMobile()`、CSS 媒体查询 | select2、MovingUI、`{{isMobile}}` |

约束：

- 不用 UA、`navigator.platform` 或 `maxTouchPoints` 推断宿主：iPadOS 报告为 Macintosh，OpenHarmony 的 UA 不含 Android。
- 用正向条件表达类别，不用 `!android && !ios` 代表桌面。为每个平台选择实现时，`switch` 列出全部平台，`default` 抛错；某个平台独有的处理用正向判断即可。
- 上游 Bowser `isMobile()` 与 CSS 媒体查询属于上游布局语义，保持不变。规则同时取决于宿主类别与窗口方向时（如聊天宽度），组合 `isMobileHost()` 与 `util/window-layout.js` 的 `isWindowPortrait()`。

`pnpm run check:platform`（已接入 `pnpm run check`）检查以上约束，报错指向本节。

## 3. 目录结构（前端集成相关）

```text
src/
├── tauri-bridge.js            # 低层 bridge：invoke/listen/convertFileSrc
├── tauri-main.js              # 新入口：只做 bootstrap
├── tauri/
│   └── main/
│       ├── bootstrap.js       # 组合根（composition root）
│       ├── context.js         # 兼容 shim（re-export `context/index`）
│       ├── context/           # Host Kernel facade + types（对外契约保持稳定）
│       ├── kernel/            # 纯逻辑（策略/计算/键生成/追踪等）
│       ├── services/          # 有状态能力（assets/thumbnails/characters/android…）
│       ├── adapters/          # 触碰 window/DOM/上游 ST 的适配层
│       ├── download-bridge.js # 同源窗口下载桥接
│       ├── http-utils.js      # URL/Body/Response 工具
│       ├── interceptors.js    # fetch/jQuery 注入
│       ├── router.js          # 轻量路由注册与分发
│       └── routes/
│           ├── system-routes.js
│           ├── settings-routes.js
│           ├── extensions-routes.js
│           ├── resource-routes.js
│           ├── character-routes.js
│           ├── chat-routes.js
│           └── ai-routes.js
└── scripts/
    ├── extensions/runtime/      # 第三方插件运行时（资源解析/模块重写/加载器）
    └── ...                    # 上游 SillyTavern 功能模块
```

## 4. 核心模块职责

### 4.1 `bootstrap.js`

- 组装模块依赖并执行初始化。
- 确保只 bootstrap 一次。
- 在 bridge 初始化后再次尝试 patch 运行时补丁（处理加载时序问题）。
- 维护对第三方可见的宿主 ABI（`window.__TAURITAVERN__`）与请求追踪 header。

### 4.2 `context.js`

- `context.js` 仅作为兼容入口（避免外部 import 路径变化）。
- 真实实现位于 `src/tauri/main/context/index.js`：作为 Host Kernel facade 组装 `kernel + services + adapters`。
- `safeInvoke` 具备可配置的 invoke 策略（dedupe / write-behind / TTL cache），集中在 `src/tauri/main/kernel/invokes/invoke-policies.js`。
- Host 侧已知的 Rust 命令名收敛为类型：`src/tauri/main/kernel/invokes/tauri-commands.js`（`TauriInvokeCommand`）。
- 与第三方直接交互的全局符号（如缩略图 helpers）属于 Public Contract（见 `docs/FrontendHostContract.md`）。

#### 4.2.1 `window.__TAURITAVERN__.api.chat`（扩展/记忆类插件 API）

> 这是 TauriTavern 独有 API 的**唯一入口**（刻意不做 alias），为扩展提供有界历史读取、后端定位、检索与持久化能力；它增强上游完整 `chat[]` 契约，不替代该契约。

- 安装位置：`src/tauri/main/api/chat.js`（在 `src/tauri/main/bootstrap.js` 中安装到 `window.__TAURITAVERN__.api.chat`）。
- 类型声明：`src/types.d.ts`（便于扩展作者用 TS/JSDoc 一键上手）。
- 详细 API 文档与适配指南：`docs/API/`。

### 4.3 `interceptors.js`

- 代理 `window.fetch`。
- 代理 `$.ajax` 并保持 Deferred/jqXHR 行为兼容。
- 只拦截本地 API 请求，其余请求透传原生实现。

### 4.4 `download-bridge.js`

- 处理所有宿主同源窗口中的浏览器式下载（如 `blob:` / `data:` / 同源 URL + `a[download]`）。
- Android 上为同源窗口中的 `<img>` 补上 `contextmenu` 默认行为（保存图片）：用户确认后，图片进入同一条导出链路。
- 页面可读的内容交给 `deliverBlob()`；远程图片交给 `deliverRemoteFile()`，由宿主下载。两者都先暂存再交付，见[文件传输](CurrentState/FileTransfer.md)。
- 不参与 API 路由判断，避免与请求拦截职责混合。

### 4.5 `router.js` + `routes/*`

- `router.js` 提供简洁注册接口：`get/post/all`。
- `routes/*` 按业务域组织，降低文件复杂度与改动冲突。

## 5. 请求注入流程

1. 前端发起 `fetch('/api/...')` 或 `$.ajax('/api/...')`
2. 拦截器通过 `router.canHandle(method, path)` 判断是否由本地路由接管
3. 命中后交给路由分发到 `routes/*`
4. 路由通过 `context.safeInvoke(...)` 调用 Rust 命令
5. 返回标准 `Response` 给前端调用方

补充：

- `/csrf-token` 在 `system-routes.js` 中返回固定 token，用于通过前端初始化流程中的 CSRF 依赖检查。
- 所有宿主接管的路由响应都会附带 `x-tauritavern-trace-id`，用于将 DevTools Network 与 console/perf-hud 关联定位问题（header 名也可从 `window.__TAURITAVERN__?.traceHeader` 获取）。

## 6. 路由分域说明

| 文件 | 负责范围 |
|------|----------|
| `system-routes.js` | ping/version/csrf 等系统基础接口 |
| `settings-routes.js` | 设置、快照、密钥、预设 |
| `extensions-routes.js` | 扩展发现、安装、更新、删除等 |
| `resource-routes.js` | 头像、背景、主题、群组等资源接口 |
| `character-routes.js` | 角色列表、创建、编辑、导入导出、重命名 |
| `chat-routes.js` | 聊天读写、搜索、最近记录、导出 |
| `ai-routes.js` | Chat Completion（OpenAI / Claude / Gemini / OpenCode） |
| `tokenizer-routes.js` | tokenizer count/encode/decode/bias 与上游本地 tokenizer 兼容路由 |

## 6.1 聊天 payload 与 DOM 边界

- 上游接管点：`src/script.js`（character chat）与 `src/scripts/group-chats.js`（group chat）。
- 统一入口：上游只 import `src/scripts/chat-payload-transport.js`，不要直接依赖 `src/scripts/tauri/chat/*`。
- 第一方加载全部楼层，可选[历史滑动按需加载](CurrentState/ChatPayload.md#21-历史滑动按需加载)。header 与消息数组分离，generation、扩展和保存共享完整、有序的 canonical `chat[]`。
- `chat_truncation` 只限制初始 DOM。Show More 从完整 `chat[]` 补挂楼层，不发起分页 I/O，不改变消息绝对索引。
- 当前聊天完整保存与 metadata 保存共用 `enqueueChatSave()` 和 transport 提交会话；transport 自身不重复入队。提交捕获独立快照并原子发布，保存期间的修改不混入本次提交。
- `saveMetadata()` 只保存 header 的 `chat_metadata`，不保存消息，也不取消待执行的完整保存。修改消息的扩展必须显式调用完整保存。
- tail/before/beforePages 只属于 `api.chat.history` 与 Agent 的显式只读查询，不参与前端当前聊天状态。
- 聊天格式遵循 [ChatPayload §1.1](CurrentState/ChatPayload.md#11-统一格式底线)；解析失败必须传播，不能提交部分历史。

保存规则与职责见 [ChatPayload](CurrentState/ChatPayload.md)；兼容 get/save 路由及事件契约见 [FrontendHostContract §4.3](FrontendHostContract.md#43-路由表public)。

## 6.2 ChatSurface 所有权与 participant

- `chat[]` 仍是完整、唯一的数据事实源；ChatSurface 只拥有 `#chat > .mes` 的当前视图投影。
- `installChatSurfaceRuntime()` 是 `script.js` 唯一的 concrete composition seam；结构/投影位于 kernel，生命周期协调位于 services，真实 DOM/scroll 写入位于 adapters。
- 外部 renderer 通过 `window.__TAURITAVERN__.api.chatSurface.registerParticipant()` 接入，不直接控制投影，也不依赖伪造消息事件。
- 异步模板通过独立的 `registerContentProcessor()` 准备显示 HTML；`content-preparation.js` 保存消息内容结果，滚动重挂载不重复求值。manifest `hooks.chatSurface` 在首次投影前完成注册，内容结果通过既有同步事务交给 participant。
- 同步与批量格式化共用一条链路，对外只交付最终 HTML；正文与 reasoning 共用 ChatSurface 内容刷新。核心通过 `finalizeMessageContent()` 等待内容提交后发送消息事件，`commitContent()` 只提交已准备内容。
- renderer 在入口只调用一次 `isManagedOwnershipRequired()`：`true` 时只启用 participant owner，`false` 时只启用原 static owner；API 是否存在和当前 DOM 形状都不能替代该决策。
- 结构 reconcile 与纯 range projection 分离：前者只在 canonical `chat[]` 结构改变时 O(N) 执行，后者携带 epoch/revision token 并保持 O(M)。
- mount lease 与 content lease 分离；streaming 中间帧只提交内容，最终帧才恢复 decorator/runtime。
- `chat_virtualization_enabled=false` 是默认值并保持上游 Show More 语义；TauriTavern Settings 的“聊天 DOM 虚拟化”开关直接绑定这个布尔值并在保存后 reload。用户显式开启后，experimental bounded 使用 TanStack 几何提交 `V ∪ T`，true tail 常驻且 `.mes` 上界为 `32 + 1`。
- TanStack symbols 由 `lib.js` vendor facade 在 composition root 注入单一 adapter；原生 ESM 源码不直接解析 bare package。virtualizer 的 `scrollToFn` 也注入既有 ChatScrollAdapter，因此 `#chat` 的物理滚动写入仍只有一个 owner。
- bounded 的 runtime candidate 由独立 grant lease 管理；viewport 滚动可以 revoke/re-grant，而不会伪造 content commit 或消息业务事件。
- runtime demand 只按 visible 与 overscan 排序；常驻 true tail 不额外预留 runtime 名额。真实全局 reflow 会保持独立 follow-tail 意图，或以稳定消息 key + clamp 后的楼内 offset 恢复锚点；普通 settings save 与 IME height-only resize 不再清空全部历史测量。
- 可由 `chat[mesid]` 推导的楼层状态由 materializer 负责；bounded DOM commit 后，私有 `syncMountedChatViewState()` 在最终测量前幂等重放 swipe/delete 等临时核心 UI。该 seam 不发送 SillyTavern 业务事件。
- 旧扩展直接删除消息 root 时，兼容桥只负责释放相应 root 的 lease；它不会接管第三方的内容写入或重编号。bounded policy 必须先通过 participant capability gate。
- Project raw API 见 `docs/API/ChatSurface.md`。

## 6.3 Theme 作用域与所有权

- `power_user.theme` 始终表示当前实际生效的命名 Theme；主题 CRUD、斜杠命令与扩展上下文继续遵循这一上游语义。
- `power_user.theme_fallback` 保存无局部绑定时的全局 Theme；系统动态主题只更新该 fallback，不覆盖当前聊天、角色或群组绑定。
- Theme 解析顺序固定为 `chat_metadata.theme -> character/group binding -> theme_fallback`。角色绑定以未经转换的 avatar filename 为键，群组绑定以 group id 为键。
- Theme 文件只保存可移植的外观快照，不包含本机角色或聊天身份。聊天绑定随 JSONL header metadata 持久化；角色与群组绑定随 settings 持久化。
- 所有生效切换必须经 `src/scripts/power-user.js` 的统一入口；Tauri appearance adapter 不模拟 `#themes` 的 DOM change 事件。

## 6.4 Chat Completion 参数管理

实现位于 `src/scripts/tauri/generation-params/`，在 `APP_READY` 后导入并挂载。参数发现、渠道支持与值的读写沿用上游设置和控件。

- 移除请求参数表示本次生成不启用该参数，保留原值；移除开关表示关闭；隐藏本地区块不改变其内容或行为。旧预设缺少 Fast Mode 字段时明确关闭。
- 请求参数的移除状态保存在预设 `extensions.tauritavern.omit_params`，本地区块显隐仅保存在设备上。旧预设保持原行为，移除范围限于可选参数，不能删除 `messages`、`model` 等结构字段。
- 最大回复长度是请求参数：移除后不发送 `max_tokens` / `max_completion_tokens`，本地按 25k 预留回复（`OMITTED_MAX_TOKENS_BUDGET`）；Claude Messages（原生、Vertex、Bedrock、Custom 共用同一 builder）与 Bedrock 自定义模板要求该字段，由后端补 25k；输出上限低于 25k 的旧模型（如 Claude 3 的 4096 / 8192）会拒绝该请求，使用时需保留回复上限。Gemini 2.5 的思考预算按回复上限的比例计算，省略时同样以 25k 为基数。单次请求显式指定的回复长度（`/gen length=`、quiet prompt 的 `responseLength`）照常发送：`TempResponseLength` 在该次生成期间撤销省略，结束后恢复。Chat Completion 的 Prompt 预算读生效设置，世界书预算与 `{{maxResponse}}` / `{{maxPrompt}}` 经 `getMaxResponseTokens()`，Prompt Manager 的超限提示与 itemized prompt 视图经所读设置的 `getEffectiveGenerationSettings()`，读到同一预留值。上下文长度只用于本地预算，不发送，移除即恢复默认 1,000,000 并隐藏，值不等于默认时自动显示。上下文上限不再按模型锁定（旧设置与预设中的 `max_context_unlocked` 迁移为 true）。
- 流式传输与推理强度（`PINNED_PRESET_KEYS`）固定在「请求参数管理」上方，折叠区块不影响它们，也不能移除：流式传输保留自身勾选框；推理强度以「自动」表示不发送，旧预设移除过的推理强度按「自动」处理。
- 其余开关显示即开启、× 即关闭，勾选框隐藏，点击标签不切换。
- 推理强度选项随当前来源与 API 格式变化，映射规则见 [NativeApiFormats](CurrentState/NativeApiFormats.md)。选择器显示实际发送的值；已存值映射后仍无法发送时，选择器显示「自动」，下方加一行说明，已存值保留。
- 每个参数旁的问号给出一句说明（`PARAM_HINTS`），点击时以 toast 重复，供触屏使用。上游已有 tooltip 的设置不再添加，只链接文档的上游问号由它替代。有说明的区块隐藏自身的长描述，嵌套子设置的描述保留。`pointer: coarse` 下问号与 × 的点击区域至少 32px。
- 滑杆（以及没有描述的数字框）收成一行数字框。Chat Completion 模式下隐藏上游的「点击滑杆数字手动输入」提示（`#clickSlidersTips`），其他 API 照常显示。
- 点击「请求参数管理」标题可折叠整个区块，折叠状态仅保存在设备上。
- Prompt 组装和生成判断使用本次生效设置，`createGenerationParameters()` 出口统一省略字段；用户显式配置的 Additional Parameters 仍拥有最终覆盖权。
- JSON 视图只编辑当前格式可用的参数，非法输入整体不应用；它不是最终请求预览，后续仍遵循渠道与模型的转换规则。

## 6.5 紧凑选择行

`src/scripts/tauri/compact-rows/` 提供「标签 | 下拉框 | 常用按钮 | ⋯」紧凑行的样式与 ⋯ 菜单。AI 响应配置面板顶部的「预设」行由 `preset-row.js` 安装。原按钮只加 `.tt-hidden` 隐藏，id 与事件处理不变，菜单项直接触发它们。

- 「预设」行顺序为：标签、下拉框、💾 保存、第三方按钮、⋯。⋯ 中依次是另存为、重命名、导入、导出预设、导出预设与配置、扩展移入的按钮、「预设绑定连接」与删除。两个导出项对应上游导出弹窗的「是否导出连接数据」两个选项（`exportOpenAIPreset({ includeConnection })`），文件格式与上游相同：「导出预设」去掉全部连接字段（上游的敏感字段都属于连接字段，因此不再询问）；「导出预设与配置」保留预设存储的连接字段，仍按上游询问是否移除敏感字段。直接点击上游 `#export_oai_preset` 仍走原来的两步弹窗。标题栏只隐藏上游已知的元素（标题文字与绑定开关）；其中没有其他可见内容时，才通过 `:has()` 整体收起，扩展加在标题栏的按钮仍然可用。
- ⋯ 菜单挂在 `body` 上并用 Popper 定位（声明 `SURFACE.None`，移动端几何防火墙不再改写其位置；菜单内的 `mousedown` / `touchstart` 不冒泡，上游「按下抽屉外即关闭抽屉」不会因此收起所在面板；Android 返回键合成的 `mousedown` 同样关闭菜单），点击开关，同一时间只开一个。外部按下、Esc、Tab、焦点移出菜单、选中菜单项、按钮所在的滚动容器或窗口滚动、窗口 resize 都会关闭菜单；其他区域的滚动（例如流式输出中的聊天区）不关闭。触屏与鼠标行为一致。`pointer: coarse` 下控件至少 36px、菜单项 40px。

### 6.5.1 扩展按钮移入 ⋯（`data-tt-overflow`）

扩展加到 Chat Completion 预设行（`#settings_preset_openai` 所在行，含其按钮栏）的按钮默认保持可见，排在 💾 之后、⋯ 之前。按钮带上 `data-tt-overflow` 属性，就改为列进该行的 ⋯ 菜单（`adoptOverflowButtons`）。Agent 资源按钮就是这样移入的。

```js
button.dataset.ttOverflow = '';
button.setAttribute('aria-label', '我的操作'); // 或 title
document.querySelector('#update_oai_preset').parentElement.append(button);
```

- 按钮由 CSS 隐藏，晚于安装注入的按钮同样生效；菜单每次打开时重新读取，按文档顺序排在两个导出项之后。
- 菜单项文字取 `aria-label`，没有时取 `title`；两者都没有的按钮不列出，并输出 `console.error`。
- 图标复制按钮内第一个 `<i>`、`<svg>` 或 `<img>`（去掉 id），都没有时用拼图图标。
- 选中菜单项即调用 `button.click()`；处于 `:disabled` 状态的按钮不列出。
- 属性只在紧凑行内生效；预设标题栏和其他位置的按钮不受影响。

「预设绑定连接」菜单项反映 `#bind_preset_to_connection` 的勾选状态。

## 7. 插件系统前端适配

### 7.1 设计目标

- 保持上游 `scripts/extensions.js` 的调用语义不变（manifest 结构、启用逻辑、依赖检查）。
- 将 Tauri 专属逻辑限制在独立 runtime 子模块，减少与上游同步冲突。
- 支持第三方插件从用户数据目录加载 JS/CSS/静态资源，不依赖 Node.js 后端。

### 7.2 模块分层

- `src/scripts/extensions.js`：插件激活编排层（发现、排序、依赖/版本检查、触发加载）。
- `src/scripts/browser-fixes.js`：上游浏览器补丁（保持与 SillyTavern 同步）。
- `src/tauri/main/compat/mobile/mobile-runtime-compat.js`：Tauri mobile 运行时 polyfills（补齐旧 WebView 缺失 JS API）。
- `src/scripts/util/window-layout.js`：移动端原生窗口快照；`src/scripts/window-backdrop.js`：第一方背景唯一写入与条带发布入口。
- `src/scripts/extensions/runtime/resource-paths.js`：扩展资源路径规范化与 third-party 判定。
- `src/scripts/extensions/runtime/tauri-ready.js`：等待 `__TAURITAVERN_MAIN_READY__`，避免 bridge 未就绪时提前加载。
- `src/scripts/extensions/runtime/third-party-runtime.js`：第三方扩展样式兼容层（legacy WebView 下为样式 URL 附加 `ttCompat=layer`，触发 Rust 端点做 `@layer` 展平；不再走前端预取/Blob 注入）。
- `src/scripts/extensions/runtime/asset-loader.js`：脚本与样式注入、超时保护、重复注入幂等控制。

### 7.3 端到端加载链路

1. `loadExtensionSettings()` 先等待 `waitForTauriMainReady()`。
2. 前端通过 `/api/extensions/discover` 获取扩展列表与类型，读取 manifest 并进入 `activateExtensions()`。
3. 对每个扩展执行 `addExtensionLocale()` + `addExtensionScript()` + `addExtensionStyle()`。
4. 当扩展为 `third-party/*` 时：
   - JS 入口脚本直接从 `/scripts/extensions/third-party/*` 加载（真实同源静态资源端点）。
   - CSS 仅在旧 WebView 不支持 `@layer` 时由 runtime 附加 `ttCompat=layer` query；由 Rust 端点返回展平后的 CSS bytes（否则仍走原始 URL）。
5. `/scripts/extensions/third-party/*` 由 Rust 协议层端点提供（WebView `on_web_resource_request` hook），统一返回 bytes + `Content-Type` + 404 语义。

### 7.3.1 当前实现结论

- 当前实现已经从“前端模拟静态文件服务”收敛为“前端只负责编排，Rust 负责 third-party 资源端点”。
- `src/scripts/extensions/runtime/third-party-runtime.js` 不再承担 JS 源码重写或伪服务器职责，主要只保留第三方样式兼容修复。
- 面向持续开发的现状说明见 `docs/CurrentState/ThirdPartyExtensions.md`；涉及实现边界或改动前，先读该文档，再决定是改前端 runtime 还是改后端资源端点。

### 7.3.2 First-party React extension

TauriTavern 自有的状态型 UI 作为 SillyTavern first-party extension 挂载 React client island。当前 Agent、MCP 与 Settings owned scope 均遵循这一基线：

- manifest、locale 和 SmartTheme CSS 仍遵循现有扩展资源契约；不引入 Next.js、独立页面 shell 或第二套路由。
- `src/index.tsx` 只负责等待 Host ABI、解析 concrete actions、创建 extension container 与 `createRoot()`。
- React presentation 只接收 strict typed initial state、actions 与 translator；不得直接 `invoke()`、操作 jQuery 或读取 Rust command 名。
- 跨窗口和长期数据继续由 Rust/Host service 拥有；局部 React state 只表示当前视图，不能成为平台事实源或持久 cache。
- 样式必须使用 `--SmartTheme*`、字体、动画与边框变量，保留 SillyTavern vanilla 视觉和用户主题覆盖能力。
- `tsconfig.ui.json`、React Hooks/TypeScript lint 与 Rstest/Testing Library 显式覆盖 Agent、MCP 和 `src/scripts/tauri/setting` 三个 owned scope；`pnpm check` 是统一验收入口。
- production 与 development 共用 `createRspackConfigs(mode)`。标准 Tauri dev server 在首次 development 编译成功后才监听，并只在成功重编译后 reload。
- `scripts/check-first-party-ui-guardrails.mjs` 为自有扩展架构限定为 React + Strict TypeScript。
当前工程基线、冻结 handle 与 bundle 数据见 `docs/CurrentState/FirstPartyUI.md`。

### 7.4 契约与约束

- third-party 扩展命名约定为 `third-party/<folder>`，前后端均按该约定解析。
- 扩展命令参数统一使用 camelCase（如 `extensionName`），避免 invoke 参数缺失。
- 客户端版本检查仍遵循上游格式：`SillyTavern:<version>:TauriTavern`，用于 `minimum_client_version` 判断。
- 拦截器是否接管请求由 `router.canHandle(method, path)` 决定，不再维护分散的路径白名单。
- `/api/extensions/branches` 返回远端 branch 的 `{ name, commit, current, label }`；`label` 当前有意为空，前端仅在非空时显示。
- `/api/extensions/switch` 接受 branch 短名并在成功时返回空 `204`；现有 reload/settings 时序保持不变，不新增 switch hook或借机触发 update hook。

### 7.5 常见问题定位

- `Extension module is not JavaScript`：
  - 通常表示拿到了 HTML 回包而非模块文件。
  - 优先检查 `/scripts/extensions/third-party/*` 是否被协议层端点正确响应（应返回 404 或 JS bytes，而不是 `index.html`）。
- `missing required key extensionName`：
  - 表示 invoke 参数命名不匹配，检查路由 body -> 命令参数映射。
- legacy WebView 样式 `@layer` 仍不生效：
  - 检查样式请求是否带 `?ttCompat=layer`；并验证 `/scripts/extensions/third-party/*` 端点返回的是 `text/css` bytes（非 404/HTML）。

### 7.6 后续开发规则

- 新增插件加载能力时，优先扩展 `src/scripts/extensions/runtime/*`，不要把 Tauri 细节回灌到 `extensions.js`。
- 新增插件 API 时，优先在 `src/tauri/main/routes/extensions-routes.js` 封装，再通过 `context.safeInvoke()` 调 Rust 命令。
- 若调整 third-party 静态资源路径约定，必须同时更新 `resource-paths.js` 与 Rust 协议层端点的前缀解析逻辑。

### 7.7 移动端插件兼容（新增）

#### 7.7.1 JS 运行时兼容（Android 旧 WebView）

- 实现位置：`src/tauri/main/compat/mobile/mobile-runtime-compat.js`。
- 入口：`src/tauri/main/bootstrap.js` 中安装（仅 Tauri mobile）。
- 行为：基础 API 仅在缺失时补齐；Android 的 Web Clipboard 写入统一映射到原生写入器；整体只执行一次。
- 当前按需补齐：
  - `Array.prototype.at`
  - `String.prototype.at`
  - `Array.prototype.findLast`
  - `Array.prototype.findLastIndex`
  - `Array.prototype.toSorted`
  - `Array.prototype.toReversed`
  - `Object.hasOwn`
  - `navigator.clipboard.writeText`（仅 Android；保留 Clipboard 对象上的其他方法）

该策略用于修复移动端第三方插件在初始化阶段出现的 `TypeError: *.at is not a function`，以及 Android WebView 拒绝 Web Clipboard 写入的问题。TauriTavern 第一方代码在所有平台直接使用 `src/tauri-bridge.js` 的原生写入器；宿主只授予 `clipboard-manager:allow-write-text`，不授予剪贴板读取能力，也不需要申请操作系统运行时权限。

#### 7.7.2 CSS `@layer` 降级（Android 旧 WebView）

- 实现位置：`src/scripts/extensions/runtime/third-party-runtime.js`（样式加载链路）。
- 触发条件：
  - 样式内容包含 `@layer`；
  - 当前 WebView 不支持 CSS Cascade Layers。
- 处理方式：
  - runtime 将样式 URL 改写为 `...?ttCompat=layer`；
  - Rust 协议层端点识别该 query 并移除 `@layer` 包裹（展平层级），返回可被旧 WebView 直接应用的 CSS。

该策略用于修复移动端插件面板（如 `TH-custom-tailwind`）样式大面积失效导致的布局错乱。

#### 7.7.3 原生内容视口与窗口背景

原生宿主拥有主 WebView 的矩形，系统栏、刘海和停靠键盘在原生层消费一次，网页按普通视口排版。前端只做两件事：`util/window-layout.js` 接收窗口快照并设置 `#bg1` 的窗口矩形；`window-backdrop.js` 的 `applyWindowBackdrop()` 是第一方背景的唯一写入点，新增背景来源接入它。只有高度变化的 resize 不触发与宽度相关的工作。

细节见 [CurrentState/MobileStyleAdaptation.md](CurrentState/MobileStyleAdaptation.md)，扩展可见的契约见 [API/Layout.md](API/Layout.md)。

#### 7.7.4 调试建议

- 若看到 `*.at is not a function`：
  - 检查是否为 Tauri mobile 会话，并确认 `window.__TAURITAVERN_MOBILE_RUNTIME_COMPAT__ === true`。
- 若 Android 复制失败：
  - 检查 `plugin:clipboard-manager|write_text` invoke 的拒绝原因；该链路不会静默回退到 Web Clipboard。
- 若插件样式错乱但 CSS 已成功请求：
  - 优先检查是否命中 `@layer` 降级分支；
  - 关注 `resolveStylesheetUrl()` 是否返回带 `ttCompat=layer` 的 URL。
- 若脚本弹窗贴顶到状态栏：
  - 所有内容一起偏移时，检查原生 WebView 矩形是否与政策避让一致；
  - 只有某个面板溢出时，检查其硬编码尺寸、负坐标或内部滚动边界。

### 7.8 嵌入式运行时（Embedded Runtime，消息内 iframe）

> 架构状态：仅保留为 static compatibility。它不是 ChatSurface bounded 路线的 runtime owner；新虚拟化开发不得继续扩展 parking/ghost/slot 体系。

该旧机制把消息内 iframe 包装成带预算、park/hydrate 与自愈的 slot，并在 static 消息重渲染时尽量避免 iframe teardown。bounded 启动会跳过该 chat owner；renderer 必须通过唯一 ChatSurface participant 接入，两套 owner 不得并存。

当前落地点（代码）：

- 安装入口：`src/tauri/main/services/embedded-runtime/install.js`
  - `bootstrap.js` 在 main ready 后按 bootstrap mirror 决定是否加载；在 `APP_READY` 后安装 chat adapters。
- Manager 与 profiles：`src/tauri/main/services/embedded-runtime/*`
  - 全局调试入口：`globalThis.__TAURITAVERN_EMBEDDED_RUNTIME__`
  - 配置来源：`tauritavern-settings.embedded_runtime_profile`
  - bootstrap mirror：`localStorage tt:embeddedRuntimeProfile = 'off' | 'auto' | 'compat' | 'mobile-safe'`
  - 旧版 `localStorage tt:runtimeProfile` 仅用于迁移
- DOM detectors：`src/tauri/main/adapters/embedded-runtime/*-runtime-adapter.js`
  - 已支持：JS-Slash-Runner（`.TH-render`）与 LittleWhiteBox（`.xiaobaix-iframe-wrapper`）。
- 消息写入 facade：`src/scripts/tauri/message/mes-text-write.js`
  - 上游调用点统一依赖 facade；`off` 时直接恢复普通 `.mes_text` HTML 写入语义。
- 渲染事务：`src/tauri/main/adapters/embedded-runtime/message-render-transaction.js`
  - 作为 facade 在 ER 开启时的底层实现，避免把 iframe runtime 当成普通 DOM 反复销毁重建。

当前边界说明：

- 已纳入管控：**消息内 iframe runtime**（JSR/LWB）。
- 暂不纳入：面板类 runtime 的 park（目前依赖浏览器本身回收即可）。
- 上述边界只描述 legacy profile 的当前事实，不是后续 bounded 架构的扩展清单。

更多“当前如何工作/哪些契约不能破坏/回归点”见：`docs/CurrentState/EmbeddedRuntime.md`。

## 8. 兼容层策略

- `src/tauri-main.js`：新主入口（推荐）。
- 新开发统一集中在 `src/tauri/main/*`，避免重复实现与多处注入链路并存。

## 9. 如何新增一个 Tauri 注入接口

1. 在 Rust 后端新增/确认命令（`src-tauri/crates/tauritavern/src/presentation/commands/*`）。
2. 若宿主层会调用该命令，将命令名加入 `src/tauri/main/kernel/invokes/tauri-commands.js`（`TauriInvokeCommand`，避免字符串拼写漂移）。
3. 若该命令为高频/可合并写入的调用，按需在 `src/tauri/main/kernel/invokes/invoke-policies.js` 增加/调整策略（dedupe / write-behind）。
4. 在 `src/tauri/main/routes/` 对应业务域中新增路由：路由层禁止直接引用 `window`，需要浏览器能力时下沉到 `adapters/` 或 `services/`。
5. 路由内只做参数校验、DTO 组装、`context.safeInvoke` 调用；错误直接暴露（避免 silent fallback）。
6. 保持返回结构稳定（状态码 + JSON 结构），避免破坏上游前端调用假设。
7. 跑 `pnpm run check`，确保完整工程门禁通过。

## 10. 调试与验证

建议最小验证流程：

1. `pnpm run check`
2. `pnpm run dev`
3. 启动后确认：
   - 首屏加载正常
   - 不再出现 CSRF 初始化错误
   - 角色/聊天/设置等核心接口可用

如需快速定位问题：

- 查看 DevTools 中请求是否命中本地注入路径。
- 查看控制台 `invoke` 报错信息与路由返回状态码。
- 检查对应 `routes/*` 是否遗漏请求字段映射。

### 10.1 轻量性能仪表（Perf HUD）

用于快速定位移动端/低端机型的主线程卡顿、DOM 膨胀、以及 invoke 热点。

- 默认关闭。
- 启用（需 reload 才能抓启动打点）：
  - 控制台：`localStorage.setItem('tt:perf','1'); location.reload();`
  - 或 URL：`?ttPerf=1`
- 启用后等待就绪：`await window.__TAURITAVERN_PERF_READY__`
- 常用导出命令：
  - `window.__TAURITAVERN_PERF__.downloadReport()` 下载 JSON（便于交给 AI 分析）
  - `window.__TAURITAVERN_PERF__.exportJson({ includeResources: true })` 直接拿到 JSON 字符串
  - `await window.__TAURITAVERN_PERF__.copyReport()` 通过原生写入器复制到剪贴板
- HUD 操作：拖动标题栏移动（位置持久化），点击标题栏展开/收起；桌面端可用 `Ctrl+Alt+P` 切换开关。

## 11. 工程守护（Guardrails + 类型检查）

目标：把宿主层（`src/tauri/main/*`）限制在**可长期维护**的规模与依赖形态，避免再次回到单文件膨胀与隐式耦合。

- 一键检查：`pnpm run check`（宿主边界、类型、架构边界、全部测试与 Clippy）。
- Guardrails（`scripts/check-frontend-guardrails.mjs`）：
  - 行数预算：默认单文件 `<= 500` 行；关键聚合文件受 `scripts/guardrails/frontend-lines-baseline.json` 的基线约束。
  - 依赖边界：`kernel/ports` 不得 import `services/routes/adapters`；`services` 不得 import `routes`。
  - 路由契约：`src/tauri/main/routes/*` 禁止直接引用 `window`（需要触碰浏览器/DOM/上游 ST 时，新增 `adapters/*`）。
- 类型检查（`tsc -p tsconfig.host.json`）：
  - `strict` + `exactOptionalPropertyTypes` + `noUncheckedIndexedAccess` 等强约束。
  - JS 文件默认不强制检查；需要在文件头加 `// @ts-check` 并配合 JSDoc（Host Kernel 目录已按此标准化）。
