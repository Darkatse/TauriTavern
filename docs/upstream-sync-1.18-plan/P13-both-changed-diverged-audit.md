# P13 both_changed_diverged 审核

生成基线：`docs/upstream-sync-1.18-plan/reports/summary.md`

- 生成时间：`2026-05-18T13:06:40.748322+08:00`
- `upstream_only_changed`: 0
- `both_changed_diverged`: 69
- 新增上游端点未覆盖：0
- Tauri command gap：0

## 审计结论

`both_changed_diverged` 不是剩余 69 个待覆盖文件，而是“上游 1.18.0 与本地都改过，且最终内容不逐字相同”。在 TauriTavern 里，这类差异必须先按契约分流：

1. 承载 Host ABI、Tauri resource/request bridge、移动 WebView、Rust 后端契约的分叉，应该保留。
2. 上游新增的前端行为语义，如果不依赖 Node 后端且不破坏 Tauri 契约，应该继续补齐。
3. 静态 model/provider/locale 文案漂移应作为低风险批次处理，不要和行为补丁混在一起。

本轮逐个审计后，建议划分为：

| 分类 | 数量 | 判断 |
| --- | ---: | --- |
| 需要进一步同步 | 15 | 有明确上游语义缺口，或低风险静态生态漂移 |
| 已同步完成或保留分叉 | 54 | 已等价同步，或差异属于 TauriTavern 长期宿主契约 |
| 合计 | 69 | 与最新报告一致 |

## 需要进一步同步

### A. 弹窗与阻塞操作语义

| 文件 | 缺口 | 建议 |
| --- | --- | --- |
| `scripts/popup.js` | 本地保留了 Tauri popup template fallback，但缺上游 `allowEscapeClose`、double-Escape force close、`placeholder`/`tooltip`、custom button tooltip/icon、`textarea`/`number`/disabled custom input。`action-loader.js` 已按 1.18 使用 `allowEscapeClose: false`，本地 `Popup` 忽略该参数会削弱阻塞 loader 语义。 | 高优先级。把上游 Popup 新能力小块合入，同时保留 `clonePopupTemplateDialog()` fallback。错误保持显式，不做静默兼容。 |

### B. Connection Manager 生成命令

| 文件 | 缺口 | 建议 |
| --- | --- | --- |
| `scripts/extensions/connection-manager/index.js` | 本地保留了 `custom-api-format` profile 字段，但缺上游 `/profile-genstream`、streaming display、stop handler、reasoning display、send button lock、`onStop`/`onComplete` closure。 | 高优先级。按上游语义补 `/profile-genstream`，保留本地 `custom-api-format`。先确认 `ChatCompletionService.processRequest(stream=true)` 与 `TextCompletionService.processRequest(stream=true)` 在 Tauri bridge 下返回契约一致。 |
| `scripts/extensions/shared.js` | `ConnectionManagerRequestService` 已存在，但本地删除了上游 `getProfileIcon()`，这会被 `/profile-genstream` 的 `StreamingDisplay` 使用。 | 和 connection-manager 同批补。只恢复 `createModelIcon` import 与 `getProfileIcon()`，不要改动现有 request service 路由边界。 |

### C. Vectors 运行时韧性与设置面

| 文件 | 缺口 | 建议 |
| --- | --- | --- |
| `scripts/extensions/vectors/index.js` | 本地已补 Workers AI / SiliconFlow embedding 与 Tauri endpoint，但缺上游 1.18 的 `summary_retries`、`summary_threshold`、`keep_hidden`、`skippedHashes`、fatal cause 分类、`Vectorize All` 进度计算修正、summary 失败跳过与重试语义。 | 高优先级。优先合运行时语义，不动本地 remote embedding endpoint 结构。失败应 toast + console 明确暴露；fatal 与可跳过错误按上游区分。 |
| `scripts/extensions/vectors/settings.html` | 缺 `vectors_keep_hidden`、`vectors_summary_retries`、`vectors_summary_threshold` UI，以及 `gemini-embedding-2-preview` 静态模型项。 | 和 `vectors/index.js` 同批补，避免设置项存在但 JS 不接线，或 JS 默认值存在但 UI 不可控。 |

### D. Chat Completion / Tool Calling 语义

| 文件 | 缺口 | 建议 |
| --- | --- | --- |
| `scripts/openai.js` | 本地有 DeepSeek native reasoning replay、Agent prompt marker、Workers AI/custom API format 分叉；上游 1.18 另有 `tool_reasoning_modes`、`tool_reasoning_mode`、tool-call recursion limit、tool reasoning forwarding UI 与 payload 语义。本地目前没有完整等价的上游 tool reasoning mode。 | 高优先级但不能机械合并。先写小设计：本地 `tool_reasoning_content` 与上游 `tool_reasoning_mode` 是否同一层语义；再补 UI/settings/payload，避免破坏 DeepSeek/Agent 契约。 |
| `scripts/tool-calling.js` | 本地已有 `tool_reasoning_content` 回传，是 DeepSeek/native reasoning 的本地契约。 | 不单独改。随 `openai.js` 设计确认是否需要和上游 tool reasoning mode 对齐。 |

### E. 主题、世界书与 Persona 用户可见行为

| 文件 | 缺口 | 建议 |
| --- | --- | --- |
| `scripts/power-user.js` | 本地 `/bgcol` 仍是旧的平均色 WIP；上游 1.18 已变为基于 `ThemeGenerator` 的主色提取、palette 生成、`force/name/bg` 参数、保存并应用主题。同时上游导出 `getThemeObject()`。 | 中优先级。同步 `/bgcol` 与 `getThemeObject` export，并确认 `scripts/util/ThemeGenerator.js` 是否已在本地同步。 |
| `scripts/world-info.js` | 本地保留 Tauri world-info flush/aux books/agent activation 分叉，但缺上游 `charUpdatePrimaryWorld()` 在清空 primary world 时移除 embedded `character_book` 的语义。 | 中优先级。只补该小语义块，保留本地 `updateAuxBooks` 和 Tauri persistence 边界。 |
| `scripts/personas.js` | 本地 Persona restore、Tauri path helper、descriptor normalize、reload after mutation 是长期分叉；上游仍有 Persona slash help、description position import、long-press/dropdown lorebook UI 等用户可见差异。 | 中低优先级。不要整文件覆盖。按“命令帮助/参数”“lorebook UI 操作”“descriptor 存储”三块小审计。 |

### F. 静态生态、欢迎页与翻译

| 文件 | 缺口 | 建议 |
| --- | --- | --- |
| `scripts/extensions/caption/settings.html` | 缺上游新增静态 multimodal model option，例如 `gpt-5.5` 系列、`gemini-3.1-flash-lite-preview`、`gemini-3.1-flash-image-preview`、`gemma-4` 系列、`glm-5v-turbo`。 | 低风险。确认后端 caption providers 已支持对应 provider path，再同步静态 option；若 provider 不支持，不暴露假入口。 |
| `scripts/textgen-models.js` | 本地保留 route-backed provider sync 与紧凑 NanoGPT provider 常量；上游 provider 常量仍有生态漂移。 | 低风险。作为 provider metadata 刷新批次处理，优先保持动态 metadata 路径；静态常量只补会影响 UI 过滤/警告的项。 |
| `scripts/welcome-screen.js` | 上游新增 recent chats display settings 等欢迎页行为；本地保留 chat input focus 与 Tauri UI 约束。 | 低优先级。先判断欢迎页是否为 Tauri 首屏关键路径，再按 recent chats 设置小块同步。 |
| `scripts/templates/welcomePanel.html` | 与 `welcome-screen.js` 配套的欢迎面板文案/控件漂移。 | 和 welcome-screen 同批处理，避免模板与 JS 状态不匹配。 |
| `locales/zh-cn.json` | 大量翻译漂移，可能覆盖新增 UI 字符串。 | 最后统一处理。先稳定功能键名，再补 locale，避免翻译反复返工。 |
| `locales/zh-tw.json` | 同上。 | 同上。 |

## 已同步完成或保留分叉

### A. 宿主入口、加载与事件 ABI

| 文件 | 判断 | 保留原因 |
| --- | --- | --- |
| `index.html` | 已同步完成，保留分叉 | Tauri bootstrap、Host ABI 注入、native 设置容器、移动布局入口。不能为追上游逐字一致破坏启动契约。 |
| `lib.js` | 已同步完成，保留分叉 | 本地 `lib.core` / `lib.optional` facade 是 bundle 与 WebView 兼容层。 |
| `script.js` | 已同步完成，保留分叉 | 请求拦截、Tauri ready gate、windowed payload、Agent generation options、generation idle gate 都是核心宿主契约。 |
| `scripts/extensions.js` | 已同步完成，保留分叉 | 扩展资源路径、manifest hook 激活、第三方扩展加载、Tauri resource bridge 属于长期 ABI。 |
| `scripts/events.js` | 已同步完成，保留分叉 | 本地将 `SETTINGS_LOADED` 加入 auto-fire，是已记录并测试的宿主事件 ABI。 |
| `scripts/st-context.js` | 已同步完成，保留分叉 | `SillyTavern.getContext().generate` 暴露 `generateSafely`，用于等待 generation idle，是本地兼容面。 |
| `scripts/loader.js` | 已同步完成，保留分叉 | 本地 loader 处理 Tauri 资源与启动时序，和 `script.js`/`extensions.js` 一起验证。 |
| `scripts/browser-fixes.js` | 已同步完成，保留分叉 | WebView/mobile browser fixes，不应按桌面浏览器上游逐字覆盖。 |
| `scripts/i18n.js` | 已同步完成，保留分叉 | 本地加载路径和 locale 边界与 Tauri 打包有关；翻译内容另在 locale 批次处理。 |

### B. CSS、移动布局与静态图标

| 文件 | 判断 | 保留原因 |
| --- | --- | --- |
| `style.css` | 已同步完成，保留分叉 | 包含 Tauri 桌面/移动 shell、WebView layout 修正和上游样式合并结果。 |
| `css/mobile-styles.css` | 已同步完成，保留分叉 | 移动安全区、键盘、抽屉和输入区几何是本地长期契约。 |
| `css/extensions-panel.css` | 已同步完成，保留分叉 | 扩展面板布局已按 Tauri UI 合并，剩余为样式差异。 |
| `css/popup.css` | 已同步完成，保留分叉 | CSS 面已可支持本地 popup fallback；真实缺口在 `scripts/popup.js` 行为层。 |
| `img/kobold.svg` | 已同步完成，保留分叉 | 静态图标差异，不影响协议语义。 |
| `img/koboldcpp.svg` | 已同步完成，保留分叉 | 静态图标差异，不影响协议语义。 |
| `img/koboldhorde.svg` | 已同步完成，保留分叉 | 静态图标差异，不影响协议语义。 |
| `img/pollinations.svg` | 已同步完成，保留分叉 | 静态图标差异，不影响协议语义。 |
| `img/scale.svg` | 已同步完成，保留分叉 | 静态图标差异，不影响协议语义。 |
| `img/textgenerationwebui.svg` | 已同步完成，保留分叉 | 静态图标差异，不影响协议语义。 |

### C. Prompt、STscript 与 Quick Reply

| 文件 | 判断 | 保留原因 |
| --- | --- | --- |
| `scripts/PromptManager.js` | 已同步完成，保留分叉 | 本地 prompt injection/Agent prompt marker 已和 1.18 prompt manager 行为合并；剩余是本地注入边界。 |
| `scripts/authors-note.js` | 已同步完成，保留分叉 | 差异与 prompt injection 模块化有关，功能面已纳入上游行为。 |
| `scripts/itemized-prompts.js` | 已同步完成，保留分叉 | 本地 itemized prompt/Agent 组合已有契约测试。 |
| `scripts/templates/itemizationText.html` | 已同步完成，保留分叉 | 和 itemized prompt 展示配套，剩余是本地文案/结构差异。 |
| `scripts/autocomplete/AutoComplete.js` | 已同步完成，保留分叉 | 保留本地移动输入与 autocomplete 生命周期修正。 |
| `scripts/slash-commands.js` | 已同步完成，保留分叉 | STscript 核心已合并；本地保留 iOS policy、safe generate、Tauri 输入焦点边界。 |
| `scripts/slash-commands/SlashCommand.js` | 已同步完成，保留分叉 | 和本地 parser/closure 契约一致。 |
| `scripts/slash-commands/SlashCommandCommonEnumsProvider.js` | 已同步完成，保留分叉 | 保留本地 enum provider 与 Tauri route-backed 数据源。 |
| `scripts/slash-commands/SlashCommandParser.js` | 已同步完成，保留分叉 | 本地移除了上游直接 `hljs` import，保持 lazy highlight/bundle 约束。 |
| `scripts/extensions/quick-reply/index.js` | 已同步完成，保留分叉 | Quick Reply 1.18 行为已合并；剩余为本地输入/生成状态边界。 |
| `scripts/extensions/quick-reply/src/QuickReply.js` | 已同步完成，保留分叉 | 同上。 |
| `scripts/extensions/quick-reply/src/QuickReplySet.js` | 已同步完成，保留分叉 | 同上。 |

### D. Provider、secrets、tokenizer 与请求契约

| 文件 | 判断 | 保留原因 |
| --- | --- | --- |
| `scripts/secrets.js` | 已同步完成，保留分叉 | OpenRouter PKCE 当前明确不支持；本地保留 MIMO、COMFY_RUNPOD、Android/Tauri secret 路径。 |
| `scripts/custom-request.js` | 已同步完成，保留分叉 | 本地 provider extraction 与 Rust backend payload 契约已合并。 |
| `scripts/textgen-settings.js` | 已同步完成，保留分叉 | TextGen 设置已对齐本地后端 route 和 provider source。 |
| `scripts/instruct-mode.js` | 已同步完成，保留分叉 | 小范围本地 prompt/template 契约差异。 |
| `scripts/preset-manager.js` | 已同步完成，保留分叉 | 本地保存路径和 route bridge 差异。 |
| `scripts/tokenizers.js` | 已同步完成，保留分叉 | 本地 tokenizer cache/windowed payload/Rust tokenization bridge 是长期契约。 |
| `scripts/utils.js` | 已同步完成，保留分叉 | 本地 strict resolver、pagination、Tauri helper 与上游通用工具合并后保留。 |

### E. Chat、背景、用户与数据持久化

| 文件 | 判断 | 保留原因 |
| --- | --- | --- |
| `scripts/backgrounds.js` | 已同步完成，保留分叉 | 背景 folder/image metadata 已同步，并增加本地 fail-fast response 校验和 payload shape 校验。 |
| `scripts/chats.js` | 已同步完成，保留分叉 | Tauri media/resource/delete bridge 与上游聊天行为合并。 |
| `scripts/group-chats.js` | 已同步完成，保留分叉 | 保留本地 group generation、resource path 与移动输入边界。 |
| `scripts/user.js` | 已同步完成，保留分叉 | P11 已同步备份 secrets 可见性提示；剩余为本地 user/avatar route。 |
| `scripts/RossAscends-mods.js` | 已同步完成，保留分叉 | 移动输入、send-on-enter 和 UI helpers 为本地长期差异。 |
| `scripts/dynamic-styles.js` | 已同步完成，保留分叉 | 本地动态 CSS 与 Tauri shell 组合已合并。 |

### F. 内置扩展运行时

| 文件 | 判断 | 保留原因 |
| --- | --- | --- |
| `scripts/extensions/assets/index.js` | 已同步完成，保留分叉 | Assets installation/style 与 hook 生命周期已同步；剩余是 Tauri resource/install route 差异。 |
| `scripts/extensions/caption/index.js` | 已同步完成，保留分叉 | 运行时 hook 已同步；静态模型列表差异单独在 settings 批次处理。 |
| `scripts/extensions/expressions/index.js` | 已同步完成，保留分叉 | Manifest hook 与 WebLLM/shared helper 已同步；剩余为本地资源路径差异。 |
| `scripts/extensions/memory/index.js` | 已同步完成，保留分叉 | Manifest hook 与 WebLLM shared helper 已同步。 |
| `scripts/extensions/regex/index.js` | 已同步完成，保留分叉 | Manifest hook 已同步；保留本地 regex preset/route 兼容。 |
| `scripts/extensions/stable-diffusion/index.js` | 已同步完成，保留分叉 | P10 已闭环 SDCPP / Workers AI / ActionLoader。残余 xAI/OpenAI image model 差异按当前后端能力保留，不暴露假能力。 |
| `scripts/extensions/tts/index.js` | 已同步完成，保留分叉 | MiniMax/TTS provider 小改动已同步；保留本地 provider bridge。 |
| `scripts/extensions/tts/minimax.js` | 已同步完成，保留分叉 | 本地 `response.clone().json()` 是错误读取改进，避免 JSON 失败后 body 被消费。 |
| `scripts/extensions/tts/system.js` | 已同步完成，保留分叉 | 系统 TTS 与本地 route/host audio 边界保持。 |

## 后续推进顺序

建议不要再按文件名顺序处理。更干净的推进顺序是：

1. `Popup` 阻塞弹窗语义：修 `allowEscapeClose`，直接保护 ActionLoader。
2. Connection Manager streaming：补 `/profile-genstream` 与 `getProfileIcon()`。
3. Vectors：补 summary retry/skip/keep-hidden 与 settings UI。
4. OpenAI tool reasoning：先设计再合并，避免与 DeepSeek/Agent reasoning 互相污染。
5. Power-user + World-info：各自小块同步用户可见行为。
6. Persona + Welcome：按 UI/命令帮助小批同步。
7. Static ecosystem + locales：最后补 model/provider option 和翻译。

每批完成后重新跑：

```bash
cargo run --manifest-path fastools/Cargo.toml -- upsync analyze \
  --base sillytavern-1.16.0/public \
  --target sillytavern-1.18.0/public \
  --local src \
  --route-dir src/tauri/main/routes \
  --command-registry src-tauri/src/presentation/commands/registry.rs \
  --out docs/upstream-sync-1.18-plan/reports
```

并至少执行：

```bash
pnpm run check:frontend
pnpm run check:contracts
cargo check --manifest-path src-tauri/Cargo.toml
git diff --check
```
