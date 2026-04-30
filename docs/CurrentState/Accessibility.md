# 无障碍系统（当前落地状态）

本文档记录 TauriTavern 当前已经落地的无障碍系统现状，用于后续维护与回归。这里描述的是当前实现快照，不是开发计划。

当前目标：

- 保持 SillyTavern 1.16.0 前端契约和事件语义，不把无障碍功能做成全局侵入式补丁
- 让主应用常用路径具备可读语义、键盘激活能力和必要的屏幕阅读器反馈
- Screen Reader Assistance 关闭后，不改变普通用户的交互体验，不保留额外播报和排序辅助 UI
- 复杂业务 UI 由拥有业务状态的模块自己实现无障碍辅助，避免全局 a11y 层直接改业务 DOM

## 1. 模块边界

### 1.1 Shared accessibility core

文件：

- `src/scripts/a11y.js`

这是 login/main 共享模块，必须保持轻量：

- 只导出 `initAccessibility()`
- 不 import `script.js`、`events.js`、`power-user.js`、popup 或主应用状态
- 只做基础语义补全：为既有选择器补 `role="button"`、`role="list"`、`role="listitem"`、`role="toolbar"`、`role="tab"`、toast `role="status"`
- 只监听 `childList`，对新增 DOM 应用规则
- 幂等初始化，重复调用不会重复安装 observer

维护约束：

- 不要把主应用逻辑塞回 `a11y.js`
- 不要在这里直接监听生成事件、聊天事件或设置项
- 不要在这里操作 Prompt Manager、Quick Reply、Regex 等业务列表顺序
- 不要从这里暴露公共 API

### 1.2 Keyboard interactable layer

文件：

- `src/scripts/keyboard.js`

该模块负责让非原生 button 的 SillyTavern 控件可以被键盘触发：

- 为 `.menu_button`、`.right_menu_button`、`.mes_button`、drawer icon、swipe button、背景项、消息内按钮等统一加 `interactable`
- 非原生控件在需要时补 `role="button"`
- 从 `title` 或 `data-tooltip` 推导 `aria-label`
- 根据 `.disabled` / `.not_focusable` 维护 `tabindex` 和 `aria-disabled`
- 全局处理 Enter / Space 激活

不会触发的情况：

- 事件目标是 `input`、`textarea`、`select` 或 contenteditable
- 按下了 Alt / Ctrl / Shift / Meta
- 目标本身已经是原生键盘控件
- 控件或祖先处于 disabled / not focusable 状态

这层与 Screen Reader Assistance 开关无关，应保持始终可用。

### 1.3 Main app semantic layer

文件：

- `src/index.html`
- `src/script.js`
- `src/scripts/utils.js`
- `src/scripts/backgrounds.js`
- `src/scripts/setting-search.js`

当前主界面语义包括：

- 顶部导航：`#top-settings-holder` 使用 `role="banner"` 和 `aria-label`
- 左侧 AI 配置区域：`#left-nav-panel` 使用 `role="region"`
- 右侧角色管理区域：`#right-nav-panel` 使用 `aria-label`
- 聊天主区域：`#sheld` 使用 `role="main"`，`#chat` 使用 `role="region"`
- 输入区域：`#send_form` 使用 `role="form"`
- 主 API 选择器、设置搜索框、关键 range 控件都有可访问名称
- 设置搜索结果写入 `#settingsSearchStatus` live region
- 背景 tab 同步 `role="tab"`、`aria-selected`、`aria-controls`、tabpanel 关系
- navbar drawer 和 inline drawer 同步 `aria-expanded` / `aria-controls`
- 消息节点使用 `role="article"`，并由 `syncMessageAccessibility()` 生成包含楼层、作者、消息类型、发送时间、swipe 状态、编辑状态的 `aria-label`
- swipe counter 使用 `aria-label="Swipe X of Y"`
- 编辑消息 textarea 使用 `aria-label="Editing message X"`
- Stop generation 按钮显示时设置 `aria-hidden=false` / `aria-disabled=false`，隐藏时设置为 true

主应用语义层采用 fail-fast：关键 DOM 缺失、结构不符合预期时直接 throw，避免静默退化。

## 2. Screen Reader Assistance

文件：

- `src/scripts/a11y/screen-reader.js`
- `src/scripts/events.js`
- `src/scripts/power-user.js`
- `src/script.js`

Screen Reader Assistance 是主应用专属功能，不属于 shared `a11y.js`。

### 2.1 设置语义

配置项：

- `power_user.screen_reader_assistance`

默认行为：

- 已有用户默认 `false`
- 首次 onboarding 默认 `true`，但用户可以在 onboarding 最底部的无障碍选项中关闭
- 设置页中的 `#screen_reader_assistance` 直接写入 `power_user.screen_reader_assistance`
- 旧的 broad flag `accessibility_mode` 会被删除，不再作为运行时契约

onboarding 中的选项由 `Popup` 的 `customInputs` 生成，位置在 persona 输入框下方、按钮上方。当前英文文案使用 low-vision users 表述，并用 `fa-universal-access` 作为提示图标。

### 2.2 生命周期

入口：

- `initScreenReaderAssistance()`
- `setScreenReaderAssistanceEnabled(value)`
- `isScreenReaderAssistanceEnabled()`

契约：

- `setScreenReaderAssistanceEnabled` 只接受 boolean，其他类型直接抛 `TypeError`
- 启用时创建唯一 live region：`#screen_reader_assistance_live_region`
- live region 属性：`role="status"`、`aria-live="polite"`、`aria-atomic="true"`
- 关闭时移除 live region，并移除所有 eventSource listener
- 状态变化后 emit `SCREEN_READER_ASSISTANCE_CHANGED`

live region 由模块私有状态持有。若需要播报但 live region 不存在，会直接 throw。

### 2.3 播报事件

Screen Reader Assistance 只播报生命周期状态，不播报用户聊天内容、prompt 内容或模型回复全文。

监听事件：

- `GENERATION_STARTED(type, params, dryRun)`
- `CHARACTER_MESSAGE_RENDERED(messageId, type)`
- `GENERATION_ENDED`
- `GENERATION_STOPPED`
- `GENERATION_FAILED`
- `ONLINE_STATUS_CHANGED`
- `CHAT_CHANGED`

生成开始播报仅允许这些类型：

- `normal`
- `regenerate`
- `continue`

以下情况不播报生成开始：

- `dryRun === true`
- `type` 是 `quiet`、`swipe`、`impersonate`、`first_message`
- `params.quiet_prompt` 存在且没有 `quietToLoud`

播报文案通过 `t` i18n：

- `AI is generating a response. Stop button is available.`
- `AI response ready.`
- `Generation stopped.`
- `Generation failed.`
- `API disconnected.`
- `API connected.`

`GENERATION_ENDED` 不直接播报 ready。它只启动一个短暂 stale timer，等待 `CHARACTER_MESSAGE_RENDERED` 确认真实可见消息已经渲染；停止、失败、切换聊天会清理生成状态，避免之后误报 ready。

## 3. 复杂 UI 排序辅助

复杂列表的排序辅助只在 Screen Reader Assistance 开启时显示。关闭后应移除或不渲染这些辅助控件，保留原本拖拽/鼠标体验。

核心原则：

- 业务状态归业务模块拥有
- a11y core 和 screen-reader 模块不直接 `insertBefore()` / `insertAfter()`
- 不从全局层调用 jQuery sortable 伪造排序
- 不靠通用 `.a11y-sort-button` 扫描全局 DOM

### 3.1 Prompt Manager

文件：

- `src/scripts/PromptManager.js`
- `src/css/promptmanager.css`

当前实现：

- `movePromptInActiveOrder(identifier, direction)` 修改 prompt order、保存 service settings、重新渲染列表
- Screen Reader Assistance 开启时渲染 up/down 控件
- 移动后写入 `sr-only` status：`Moved to position X of Y.`
- 移动后 focus 回到可用的排序控件
- 监听 `SCREEN_READER_ASSISTANCE_CHANGED` 后重新渲染

### 3.2 Quick Reply

文件：

- `src/scripts/extensions/quick-reply/index.js`
- `src/scripts/extensions/quick-reply/src/QuickReplyConfig.js`
- `src/scripts/extensions/quick-reply/src/QuickReplySet.js`
- `src/scripts/extensions/quick-reply/src/QuickReplySetLink.js`
- `src/scripts/extensions/quick-reply/src/QuickReply.js`

当前实现：

- Quick Reply set link 使用 `moveSetLink(index, direction)` 更新 set list 并重建设置 DOM
- Quick Reply item 使用 `moveQuickReply(id, direction)` 更新 `qrList`、保存、重渲染按钮和设置 DOM
- 辅助按钮只在 Screen Reader Assistance 开启时渲染
- 移动后写入模块自己的 `sr-only` status
- 移动后 focus 回到可用排序控件
- extension index 监听 `SCREEN_READER_ASSISTANCE_CHANGED` 后 unrender/rerender

### 3.3 Regex

文件：

- `src/scripts/extensions/regex/index.js`
- `src/scripts/extensions/regex/scriptTemplate.html`

当前实现：

- `moveRegexScriptWithinType(scriptId, scriptType, direction)` 修改对应类型脚本数组、保存、刷新 regex UI 和聊天影响
- 模板中有 move up/down 控件及 i18n 的 `title` / `aria-label`
- Screen Reader Assistance 关闭时移除这些控件
- 移动后写入 regex sort status，并 focus 回可用排序控件
- 监听 `SCREEN_READER_ASSISTANCE_CHANGED` 后重新加载 regex scripts

## 4. i18n 契约

用户可见或可听见的无障碍文案必须接入现有 i18n：

- 静态 DOM 属性用 `data-i18n`
- 运行时字符串用 `t\`...\``
- 当前新增无障碍 key 至少维护 `zh-cn` 和 `zh-tw`

已覆盖的典型 key：

- landmarks / region labels
- message aria-label 片段
- generation lifecycle announcements
- settings search result announcements
- keyboard sorting titles/status
- onboarding Screen Reader Assistance 文案

不要新增硬编码英文 live region 文案。新增 `t` key 时要同步 `src/locales/zh-cn.json` 与 `src/locales/zh-tw.json`，并保持 `${0}` 这类占位符一致。

## 5. 公共 API 与兼容性

当前没有公开无障碍 API：

- `getContext()` 不暴露 `a11y`
- `window.__TAURITAVERN__.api` 不暴露 accessibility/a11y namespace
- 扩展不能依赖 Screen Reader Assistance 内部状态

如果未来要给扩展提供稳定无障碍 API，应单独设计 public ABI、写入 API 文档，并补 contract tests。不要把内部模块直接挂到 SillyTavern context 上。

## 6. 关闭 Screen Reader Assistance 后的行为

关闭后仍然保留：

- shared `a11y.js` 的基础 role 补全
- `keyboard.js` 的键盘激活层
- 静态 landmarks、drawer 状态、message article 语义、settings form label 等基础语义

关闭后应移除或停止：

- `#screen_reader_assistance_live_region`
- generation / online status live announcements
- Prompt Manager / Quick Reply / Regex 的屏幕阅读器排序辅助 UI
- `SCREEN_READER_ASSISTANCE_CHANGED` 触发后的辅助 UI 渲染状态

关闭后不应发生：

- 把发送、停止、编辑等核心按钮从 accessibility tree 隐藏
- 额外 focus stealing
- 额外 console logging 用户聊天内容
- 全局 a11y 层继续操作业务列表排序

## 7. 测试与回归

主要自动化契约：

- `tests/accessibility-contract.test.mjs`

常用检查：

```bash
node --test tests/accessibility-contract.test.mjs
pnpm run check:frontend
pnpm run check:types
pnpm run check:contracts
```

该测试覆盖：

- login 仍只加载 shared a11y core，不加载主应用 screen-reader
- `a11y.js` 没有主应用 import，也不暴露额外 public surface
- `getContext()` 不暴露未文档化 a11y API
- Screen Reader Assistance 设置默认值、onboarding、保存链路
- live region 生命周期、事件过滤、停止/失败/连接状态播报
- 不泄露 prompt / message 内容到播报或 console
- landmarks、drawer、message、settings search、tab、keyboard sorting contract
- 复杂 UI 排序必须由 owner module API 完成
- `zh-cn` / `zh-tw` 翻译 key 和占位符一致

建议手动回归路径：

- 首次 onboarding：确认无障碍选项位于表单底部，文案和图标清晰
- Settings 中开启/关闭 Screen Reader Assistance：确认 live region 安装/移除，复杂排序辅助 UI 出现/消失
- 生成、停止、失败、API 断开/恢复：确认只播报生命周期状态
- Prompt Manager、Quick Reply、Regex：确认键盘排序会保存真实业务状态
- 关闭 Screen Reader Assistance 后：确认普通拖拽、点击、键盘基础操作不变

## 8. 最容易误改的点

- 不要重新引入 `accessibility_mode` 作为 broad mode
- 不要让 `a11y.js` import 主应用模块
- 不要在 generation 主流程中直接调用 screen-reader helper；通过事件语义驱动
- 不要把 streamed message body 变成 live region
- 不要播报或记录用户聊天内容、prompt 内容、模型回复全文
- 不要在 Screen Reader Assistance 关闭后保留辅助排序按钮
- 不要通过 DOM 顺序伪造业务排序；必须调用 owner module API 并保存
- 不要新增未翻译的无障碍用户文案
- 不要静默忽略关键 DOM 缺失；owner module 代码应 fail fast

## 9. 已知边界

- 当前不是完整的屏幕阅读器 UX 体系，只覆盖主工作流和高风险控件
- shared `a11y.js` 仍是上游风格的基础规则层，不应继续扩张
- Popup/focus trap 沿用现有 Popup contract，没有单独重写
- 第三方扩展没有稳定 a11y API
- 新增语言时需要补齐对应 locale；当前自动化只强制检查简体中文和繁体中文
