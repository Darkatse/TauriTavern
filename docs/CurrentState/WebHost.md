# Web Host

Web Host 是给浏览器嵌入方用的 TT 宿主实现，位于 `src/tauri/web-host/`。原生应用里由 Tauri 提供的那一侧，即 `window.__TAURI__` 与宿主身份 `__TAURITAVERN_HOST__`，在浏览器里由它提供。目前唯一的使用方是 [Tavern Studio](https://github.com/Darkatse/TavernStudio)，它在预览中运行 TT 前端。原生应用不加载这些文件。契约等级为 Project Contract。

## 平时开发不用考虑它

TT 的前端代码不知道自己运行在哪个宿主上，也不需要为 Web Host 写任何分支。只有下面三种情况需要改这里：

- `tests/browser/web-host-startup.test.mjs` 失败：启动路径上出现了 Web Host 还不支持的命令，或者测试后端收到了它没有准备的请求。在 `src/tauri/web-host/` 补上映射，必要时给 `tests/browser/web-host-fixture.mjs` 补上对应的端点。
- `tt-application` 的 `web_host_defaults` 测试失败：TT 设置的默认值或 DTO 变了，按"维护"一节重新生成默认设置。
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
  - `backend.handle({ method, url, headers, body })` 按 ST 的 HTTP 语义处理请求，返回 `{ status, headers, body }`。请求体和响应体都是 JSON 字符串，GET 的请求体为 `null`。Web Host 直接调用它，不经过 `fetch`，所以请求不会再被 TT 的拦截器拦截。非 2xx 的响应会变成错误，错误信息包含方法、路径、状态码和响应体。
  - `store.load()` 与 `store.save(value)` 读写 Web Host 自己的一份文档，里面是 TT 设置的 DTO。嵌入方原样保存它，不需要理解其内容。
- 后端数据和 `store` 都归嵌入方所有。TT 页面自己的模板和默认设置文件，经安装时保存下来的原始 `fetch` 读取。
- 返回的 `host.emit(name, payload)` 向 `__TAURI__.event.listen` 注册的监听器投递事件，回调收到的形状与 Tauri 相同：`{ event, id, payload }`。

## 命令

下表来自桌面与 Android 身份的实际启动。

| 命令 | 对应的后端请求或行为 |
| --- | --- |
| `wait_for_backend_ready`、`backend_error_bridge_ready` | 立即就绪；没有待投递的原生错误 |
| `get_client_version` | `GET /version` |
| `get_tauritavern_settings` | 默认设置与 `store` 文档合并，见"实现要点" |
| `get_sillytavern_settings`、`get_personas` | `/api/settings/get`，转为 TT 的形状 |
| `get_bootstrap_metadata` | `/api/characters/all`、`/api/groups/all`、`/api/avatars/get`、`/api/secrets/read` |
| `get_character_chats_by_id` | `/api/characters/chats`，转为 TT 的聊天列表 |
| `get_chat_payload_path`；`plugin:fs\|open`、`plugin:fs\|fstat`、`plugin:fs\|read`；`plugin:resources\|close` | `/api/chats/get`，见"聊天读取" |
| `get_all_backgrounds`、`get_background_folders`、`get_all_background_metadata` | `/api/backgrounds/all`、`/api/backgrounds/folders`、`/api/image-metadata/all` |
| `get_extensions` | `/api/extensions/discover` |
| `read_frontend_template` | 读取页面的 `/scripts/templates/<name>` |
| `get_world_info` | `/api/worldinfo/get`；打开聊天时会预读关联的世界书 |
| `begin_settings_commit`、`append_settings_commit_chunk`、`finish_settings_commit`、`abort_settings_commit` | 最终写入 `/api/settings/save`，见"设置提交" |
| `get_window_snapshot` | `null`：视口由浏览器管理 |
| `devlog_append_frontend_logs` | 直接确认：日志留在前端采集器里，浏览器里没有原生日志文件 |

其余命令和 `core.convertFileSrc` 一律以 `{ Unsupported: message }` 拒绝：
- 还没有支持的：会话中的写入（TT 设置更新、聊天与角色写入、主题保存）和生成。
- 不在范围内的：agent、MCP、同步、扫码、原生分享与导出、剪贴板、外部打开、更新安装、extension store。
- token 计数不支持，ST 会改用自己的估算。
- 桌面窗口的 `isFullscreen()` 返回 `false`，`setFullscreen()` 不支持。

## 实现要点

**设置提交**。启动本身就会保存一次 ST 设置，所以提交协议已经实现。
- revision 是后端当前设置字符串的 SHA-256。比较 revision 与写入在同一次同步调用里完成；两个页面先后基于同一版本保存时，后到的一方收到 `{ Conflict: message }`。
- 提交里的 `power_user.personas` 与 `persona_descriptions` 只包含有变化的 Persona，没有提到的不表示删除。Web Host 保留后端里其余的 Persona，用提交的记录整条替换对应的 ID，再保存完整的 ST 设置。Persona 与 ST 设置之间的转换和前端共用 `tauri/main/kernel/persona-projection.js`。
- 分块编码与原生相同：Android 用 `chunk-encoding: base64`，其他平台直接传字节。协议常量与内核共用 `tauri/main/kernel/byte-protocol.js`。

**聊天读取**。后端返回的消息记录编码成 JSONL，再经 `plugin:fs` 那组命令读取；每次读取的响应末尾带 8 字节大端长度，与原生相同。ST 用空对象 `{}` 表示聊天不存在：`allowNotFound` 为真时返回空路径，否则以 `{ NotFound }` 拒绝。

**角色**。`get_bootstrap_metadata` 保留后端给出的 `shallow` 标记。后端返回浅角色时，TT 会调用 `get_character`，这个命令目前不支持。

**TT 设置**。`get_tauritavern_settings` 把默认设置与 `store` 里的文档合并后返回，后来新增的字段由默认值补上。读取不会写入 `store`。

## 维护

默认设置由 Rust 生成：

```sh
cargo run --quiet --manifest-path src-tauri/Cargo.toml -p tt-application --example web_host_defaults > src/tauri/web-host/default-settings.json
```

`tt-application/tests/web_host_defaults.rs` 检查这个文件与 Rust `Default` 转成的 DTO 一致。不要在 JS 里手写默认值。

浏览器测试都经 `tests/browser/runtime.mjs` 用 Web Host 启动，后端是 `tests/browser/web-host-fixture.mjs` 里的内存实现。`web-host-startup.test.mjs` 在桌面与 Android 身份下检查完整启动：渲染聊天、预读关联世界书、启动时的设置保存不丢失 Persona；另有一个用例检查聊天不存在时的读取语义。其他浏览器测试需要额外的命令时，在测试里包装 `window.__TAURI__.core.invoke`。
