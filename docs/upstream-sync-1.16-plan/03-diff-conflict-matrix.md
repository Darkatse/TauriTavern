# 03. 差异与冲突矩阵

## 1. 统计快照（本地采样）

采样口径：对比 `sillytavern-1.15.0/public`、`sillytavern-1.16.0/public`、`src/`，并过滤 `scripts/extensions/third-party/JS-Slash-Runner/*` 噪声目录。

- 上游 1.15 -> 1.16 变更文件：**97**
- 本地相对 1.15 改造文件：**300**
- 交集（上游变更且本地也改）：**23**
- 上游变更但本地未改：**74**（优先自动同步）

## 2. 直接冲突文件（23，优先级最高）

1. `css/extensions-panel.css`
2. `index.html`
3. `locales/zh-cn.json`
4. `script.js`
5. `scripts/autocomplete/AutoCompleteNameResultBase.js`
6. `scripts/autocomplete/AutoCompleteOption.js`
7. `scripts/backgrounds.js`
8. `scripts/extensions.js`
9. `scripts/extensions/tts/coqui.js`
10. `scripts/group-chats.js`
11. `scripts/itemized-prompts.js`
12. `scripts/macros/engine/MacroRegistry.js`
13. `scripts/personas.js`
14. `scripts/power-user.js`
15. `scripts/secrets.js`
16. `scripts/slash-commands/SlashCommandClosure.js`
17. `scripts/slash-commands/SlashCommandParser.js`
18. `scripts/templates/welcomePanel.html`
19. `scripts/util/AccountStorage.js`
20. `scripts/utils.js`
21. `scripts/welcome-screen.js`
22. `scripts/world-info.js`
23. `style.css`

## 3. 注入相关关键冲突

## 3.1 `index.html`

- 本地：`script.js` 改为 `init.js` 引导，新增 `modulepreload`（`dist/lib.bundle.js`、`tauri-main.js`、`script.js`）。
- 风险：若直接覆盖上游 1.16，会丢失 Tauri 启动链。

## 3.2 `script.js`

已观测到的关键差异：

- 本地新增 Tauri 客户端版本桥接与兼容版本常量
- 本地接入 `chat-payload-transport` 高性能链路
- 上游 1.16 新增 `APP_INITIALIZED` 事件（本地当前未发）
- 上游 1.16 将 `chatLoaded` 语义切换到 `event_types.CHAT_LOADED`（本地仍为字符串事件）

结论：`script.js` 必须手工精细合并，不能整文件替换。

## 4. 上游新增静态接口（1.16 相对 1.15）

静态提取结果显示新增 **9** 条接口，当前 Tauri 路由全部未覆盖：

1. `/api/backends/chat-completions/multimodal-models/moonshot`
2. `/api/chats/group/info`
3. `/api/image-metadata/all`
4. `/api/openai/nanogpt/models/embedding`
5. `/api/sd/comfy/rename-workflow`
6. `/api/sd/sdcpp/generate`
7. `/api/sd/sdcpp/ping`
8. `/api/sd/zai/generate-video`
9. `/api/volcengine/generate-voice`

说明：这是静态字符串口径，实施时仍需动态调用验证。

## 5. 风险分级

- `P0`（阻塞启动/主流程）：`index.html`、`script.js`、`scripts/extensions.js`
- `P1`（主要功能）：`group-chats.js`、`world-info.js`、`power-user.js`、`utils.js`
- `P2`（体验与边缘）：`welcome-screen.js`、样式与部分本地化文件

## 6. 执行策略映射

1. 先同步 74 个无冲突文件（低风险批量）。
2. 对 23 个冲突文件分 P0/P1/P2 三波合并。
3. 同步后补齐新增 9 接口（最少保证“可调用且可降级”）。

