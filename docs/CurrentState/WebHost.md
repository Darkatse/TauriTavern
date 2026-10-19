# Web Host

Web Host 是给浏览器嵌入方用的 TT 宿主实现，位于 `src/tauri/web-host/`。原生应用里由 Tauri 提供的那一侧，即 `window.__TAURI__` 与宿主身份 `__TAURITAVERN_HOST__`，在浏览器里由它提供。目前唯一的使用方是 [Tavern Studio](https://github.com/Darkatse/TavernStudio)，它在预览中运行 TT 前端。原生应用不加载这些文件。契约等级为 Project Contract。

## 平时开发不用考虑它

TT 的前端代码不知道自己运行在哪个宿主上，也不需要为 Web Host 写任何分支。只有下面三种情况需要改这里：

- `tests/browser/web-host-startup.test.mjs` 或 `web-host-session.test.mjs` 失败：Web Host 还不支持的命令，或测试后端没有准备的请求，让启动、会话写入或生成出错了。在 `src/tauri/web-host/` 补上映射，必要时给 `tests/browser/web-host-fixture.mjs` 补上对应的端点。
- `tt-application` 的 `web_host_defaults` 测试失败：原生默认值或 DTO 变了，按“维护”一节重新生成默认状态。
- 修改了下表中命令的 DTO 或协议，例如设置提交、聊天文件读取、Persona 增量：同步修改这里的映射。

## 安装

```js
import { installWebHost } from '/tauri/web-host/install.js';

const host = installWebHost({
    identity: { platform: 'windows', kind: 'desktop' },
    backend: { handle(request) { /* 同步返回 ST HTTP 响应 */ } },
    store: {
        load() { /* 同步返回之前保存的文档，第一次为 null */ },
        save(value) { /* 同步保存文档 */ },
    },
});
```

- 在 `init.js` 之前调用。调用返回时，`window.__TAURI__` 与冻结的宿主身份都已就绪。`identity` 只能是真实平台，取值与原生相同。
- TT 页面可以运行在嵌入方的 iframe 里。
- 三个端口都同步返回：
  - `backend.handle({ method, url, headers, body })` 按 ST 的 HTTP 语义处理请求，返回 `{ status, headers, body }`：
    - JSON 请求与响应使用字符串，GET 的请求体为 `null`；
    - 角色编辑使用纯数据表单 `{ kind: 'form', entries: [name, value][] }`，只传文本字段；
    - 流式响应见“生成流”；
    - 非 2xx 响应会变成错误，错误信息包含方法、路径、状态码和响应体，所以这时的 `body` 必须是字符串。
  - `store.load()` 与 `store.save(value)` 读写 Web Host 自己的一份文档，里面是 TT 设置的 DTO。嵌入方原样保存它，不需要理解其内容。
- 后端数据和 `store` 都归嵌入方所有。页面模板与 `default-state.json` 按静态文件读取，嵌入方需原样提供 vendor 镜像中的这些文件。
- 返回的 `host.emit(name, payload)` 向 `__TAURI__.event.listen` 注册的监听器投递事件，回调收到的形状与 Tauri 相同：`{ event, id, payload }`。
- TT 在页面隐藏时（`pagehide`、`visibilitychange:hidden`）异步保存设置与聊天，这些保存要求页面仍在文档中。

## 命令

下表覆盖实测的三条路径：启动到聊天、会话写入、Replay 生成。未注明方法的请求都是 POST。

| 命令 | 对应的后端请求或行为 |
| --- | --- |
| `wait_for_backend_ready`、`backend_error_bridge_ready` | 立即就绪；没有待投递的原生错误 |
| `get_client_version` | `GET /version` |
| `get_tauritavern_settings`、`update_tauritavern_settings` | 读取及部分更新 `store` 文档，见“TT 设置” |
| `get_sillytavern_settings`、`get_personas` | `/api/settings/get`，转为 TT 的形状 |
| `read_secret_state` | `/api/secrets/read`，连接按钮刷新密钥状态 |
| `get_character` | `/api/characters/get`，见“角色” |
| `get_bootstrap_metadata` | `/api/characters/all`、`/api/groups/all`、`/api/avatars/get`、`/api/secrets/read` |
| `get_character_chats_by_id` | `/api/characters/chats`，结果须为数组，没有聊天时为 `[]`；ST 这时返回的 `{ error: true }` 会被拒绝 |
| `get_chat_payload_path`；`plugin:fs\|open`、`plugin:fs\|fstat`、`plugin:fs\|read`；`plugin:resources\|close` | `/api/chats/get`，见"聊天读取" |
| `get_all_backgrounds`、`get_background_folders`、`get_all_background_metadata` | `/api/backgrounds/all`、`/api/backgrounds/folders`、`/api/image-metadata/all` |
| `get_extensions` | `GET /api/extensions/discover`，把 ST 的 `type` 转回原生 `extension_type` |
| `read_frontend_template`、`read_frontend_extension_template` | 读取页面的模板与系统扩展模板 |
| `open_extension_store_entry` | `null`，全新安装尚无扩展私有数据 |
| `list_agent_profiles` | Rust 生成的默认 Writer 列表 |
| `list_mcp_servers`、`list_skills` | 全新安装的空列表 |
| `register_agent_extension_tool` | 确认接收页面启动时的工具声明；运行 Agent 时拒绝 |
| `database_handle` | `flushAll` 返回 `null`：没有打开的数据库；其他请求拒绝 |
| `get_world_info` | `/api/worldinfo/get`；打开聊天时会预读关联的世界书 |
| `begin_settings_commit`、`append_settings_commit_chunk`、`finish_settings_commit`、`abort_settings_commit` | 最终写入 `/api/settings/save`，见"设置提交" |
| `begin_chat_commit`、`append_chat_commit_chunk`、`finish_chat_commit`、`abort_chat_commit` | 完整聊天、metadata 与 metadata extension，最终写入 `/api/chats/save` |
| `save_theme` | `/api/themes/save` |
| `save_quick_reply_set` | `/api/quick-replies/save` |
| `update_character_card_data` | 卡片 DTO 转为 `/api/characters/edit` 的文本表单，随后读取 `/api/characters/get` 返回更新后的 DTO |
| `merge_character_card_data` | `/api/characters/merge-attributes`，随后读取更新后的 DTO |
| `generate_chat_completion` | `/api/backends/chat-completions/generate`，非流式 JSON 响应 |
| `get_chat_completions_status` | `/api/backends/chat-completions/status`，连接与模型列表 |
| `start_chat_completion_stream`、`read_chat_completion_stream`、`close_chat_completion_stream` | 同一生成端点的 SSE 流，转为 `afterSeq` 会话 |
| `cancel_chat_completion_generation` | 确认接收；非流式请求同步完成，没有可取消的请求 |
| `chat_history_generation_started`、`chat_history_generation_finished` | 确认接收；Session 的聊天写入即记录，不建立原生历史仓库 |
| `get_notification_permission_state` | `denied`：预览不拥有原生系统通知能力 |
| `get_window_snapshot` | `null`：视口由浏览器管理 |
| `devlog_append_frontend_logs` | 确认接收；日志留在前端采集器里，浏览器里没有原生日志文件 |

其余命令和 `core.convertFileSrc` 一律以 `{ Unsupported: message }` 拒绝：
- 不在范围内的：Agent 运行、MCP 操作、同步、扫码、原生分享与导出、剪贴板、外部打开、更新安装、extension store 写入、群聊与 cold swipe 资源。
- 调用方自己能处理失败的命令不实现，例如 token 计数（ST 改用估算）和启动时的更新检查（跳过）。
- 桌面窗口的 `isFullscreen()` 返回 `false`，`setFullscreen()` 不支持。

## 实现要点

**设置提交**
- revision 是后端当前设置字符串的 SHA-256。比较 revision 与写入在同一次同步调用里完成；两个页面先后基于同一版本保存时，后到的一方收到 `{ Conflict: message }`。
- 提交里的 `power_user.personas` 与 `persona_descriptions` 只包含有变化的 Persona，没有提到的不表示删除。Web Host 保留后端里其余的 Persona，用提交的记录整条替换对应的 ID，再保存完整的 ST 设置。Persona 与 ST 设置之间的转换和前端共用 `tauri/main/kernel/persona-projection.js`。
- 分块编码与原生相同：Android 用 `chunk-encoding: base64`，其他平台直接传字节。协议常量与内核共用 `tauri/main/kernel/byte-protocol.js`。
- 设置和聊天共用 `protocols/byte-commit.js` 的分块会话机制。

**聊天提交**。完整 payload 编码为 JSONL；metadata 提交只替换 header 的 `chat_metadata`；metadata extension 替换对应命名空间，`null` 删除它。TT 在已有 integrity 时要求新值完全匹配，省略也拒绝，不匹配返回 `{ BadRequest: 'integrity' }`。完整 payload 的 `force` 原样传给 ST，metadata 提交用 `false`；不强制保存时，ST 会拒绝覆盖首行已损坏的聊天文件。

**聊天读取**。后端返回的消息记录编码成 JSONL，再经 `plugin:fs` 那组命令读取；每次读取的响应末尾带 8 字节大端长度，与原生相同。ST 用空对象 `{}` 表示聊天不存在：`allowNotFound` 为真时返回空路径，否则以 `{ NotFound }` 拒绝。

**角色**。`get_bootstrap_metadata` 保留后端给出的 `shallow` 标记，浅卡片的 `json_data` 为 `null`。`get_character` 与编辑后的回读得到完整卡片，`json_data` 是 ST 返回的原始卡片文本。ST 读卡失败时仍返回 200 和一个不含 `json_data` 的空壳，Web Host 把它当作错误。完整卡片更新转回 ST 的文本表单，保留 `json_data` 中的其他字段；头像文件与裁剪不在范围内。

**TT 设置**。读取时合并默认值与 `store` 文档，新增字段由默认值补上。更新遵循 Rust DTO 的 `Option` 语义：未提供或为 `null` 的普通字段保留；嵌套段落按字段更新，数组替换。`request_proxy` 是完整 DTO，整段替换；`updates.startup_popup.dismissed_release_token` 允许 `null` 清除。更新在同一任务里读取、合并并保存。

**系统扩展**。初始化时的读取返回全新安装的结果，工具声明确认接收；真正使用这些能力时才拒绝。

**生成流**。后端同步返回响应头，`body` 是 `ReadableStream<Uint8Array>`，内容为标准 SSE。该流是可转移对象；同源调用可直接返回它，经结构化克隆转交时必须把流加入 transfer list。取消和关闭通过 reader 的 `cancel()` 传回后端。

SSE 解析使用上游 `scripts/sse-stream.js`。每次读取一个事件，保留尚未确认的批次；相同 `afterSeq` 重放该批次，包括尚在阻塞的读取。结束和错误产生终止事件，关闭流释放阻塞读取。

## 维护

`default-state.json` 中的设置 DTO 与默认 Writer 列表由 Rust 生成：

```sh
cargo run --quiet --manifest-path src-tauri/Cargo.toml -p tt-application --example web_host_defaults > src/tauri/web-host/default-state.json
```

`tt-application/tests/web_host_defaults.rs` 用同一段代码重新生成，并检查与提交的文件一致。

浏览器测试通过真实前端、系统扩展和最小第三方扩展验证启动与会话路径，检查扩展激活结果及测试期间的错误；Studio 的实际 Session Backend、JSR 与 Replay 联调由 Studio 验收。
