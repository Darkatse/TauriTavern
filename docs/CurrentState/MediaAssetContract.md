# 媒体资源契约（Media Assets）现状

本文档记录当前**已经落地**的“浏览器原生媒体加载契约”：`<video>` / `<audio>` 在桌面与移动端对用户静态资源端点（尤其是 `/backgrounds/*`）的请求方式，以及宿主目前承诺的响应语义。

> 实现位置：`src-tauri/src/presentation/web_resources/user_data_endpoint.rs`

---

## 1. 范围与结论

目标（Public Contract）：

- 上游 SillyTavern 与第三方扩展可以把 `/backgrounds/*` 等路径当作“普通 HTTP 资源端点”使用（子资源加载 + Range）。
- 媒体文件（`video/*` / `audio/*`）必须满足浏览器媒体管线的最小网络契约：**支持 `Range`（单范围）并返回 `206 + Content-Range`**。

涉及端点（由 `user_data_endpoint.rs` 提供）：

- `/backgrounds/*`（图片背景 + 视频背景）
- `/assets/*`、`/user/files/*`（可能承载音视频/下载内容）
- 以及同一实现覆盖的其它用户静态资源：`/characters/*`、`/User Avatars/*`、`/user/images/*`

---

## 2. 端点基础语义（全平台）

这些端点必须能被浏览器原生子资源加载（`<img src>` / `<video src>` / `CSS url()`），且 dev/prod 语义一致：

- 仅接受 `GET` / `HEAD` / `OPTIONS`，其他方法返回 `405`
- 未命中返回真实 `404`（不回退到 `index.html`）
- `Content-Type` 必须与文件类型匹配（基于扩展名推断）
- `Cache-Control: no-store`
- `Accept-Ranges: bytes`

---

## 3. Range 契约（单范围）

支持的 `Range` 形态（仅单范围）：

- `Range: bytes=<start>-<end>`
- `Range: bytes=<start>-`
- `Range: bytes=-<suffixLen>`

响应语义：

- 满足范围：返回 `206 Partial Content`
  - `Content-Range: bytes <start>-<end>/<total>`
  - `Content-Length: <rangeLen>`
- 非法/不满足：返回 `416 Range Not Satisfiable`
  - `Content-Range: bytes */<total>`

显式不支持：

- multi-range（例如 `bytes=0-1,2-3`）会按“非法 Range”处理并返回 `416`。

---

## 4. Android WebView 差异与当前 workaround（视频背景）

现象（历史问题）：

- Android WebView 对 `video/mp4` 的请求序列通常包含多个 Range（例如 `bytes=0-`、`bytes=131072-`、尾部 Range 等）。
- 在 Tauri mobile 的资源拦截链路中（`shouldInterceptRequest`），Android WebView 会对**拦截返回的响应流**再次应用请求的 Range 语义。
- 若宿主已经按 Range 做了 seek/slice，再遇到非 0 起点 Range，会出现“再次 seek 导致不可满足”的情况，表现为 `416` 或请求被快速取消，最终 `<video>` 卡在 `HAVE_NOTHING`，无法进入 `loadedmetadata`。

当前已落地 workaround（仅 Android + 背景视频 + 非 0 Range 起点）：

- 匹配条件：
  - 平台：Android（WebView 拦截链路）
  - 路径：`/backgrounds/*`
  - MIME：`video/*`
  - Range：`start != 0`
- 响应策略：
  - 返回 **`206` + 正确的 `Content-Range/Content-Length`（对齐请求的范围）**
  - 但 body 提供**完整文件 bytes**，让 WebView 自己在流上执行 Range（skip）并喂给媒体管线

这保持了“对媒体管线可观察的 HTTP 契约”，并避开 Android WebView 的二次 Range 应用问题。

已知代价：

- 对这些请求会读取完整文件到内存；背景视频过大时可能带来额外内存/IO 压力。

---

## 5. 回归与诊断要点

最小回归探针（桌面/移动均适用）：

- `fetch('/backgrounds/<file>.mp4', { headers: { Range: 'bytes=0-1' } })` 应返回 `206` 且包含 `Content-Range`

Android 端额外关注：

- 对 `Range: bytes=131072-` 等非 0 起点 Range，应依然返回 `206` 且包含 `Content-Range`
- 若 `<video>` 长时间停留在 `readyState=HAVE_NOTHING`，优先排查 Range 契约是否被破坏（`206`、`Content-Range`、以及是否出现快速 canceled）

当需要确认媒体编码兼容性（解码层问题）：

- 使用 `ffprobe` 检查视频编码/音频声道布局（Android 上 AAC 5.1 可能存在兼容风险）。

