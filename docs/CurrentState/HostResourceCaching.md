# Host Resource 表示与缓存契约

本文记录 TauriTavern 自有 Host Resource 的当前表示、条件请求和平台交付契约。普通外部 HTTP/HTTPS 资源不属于本模块，由原生 WebView 网络栈处理。

## 1. 覆盖范围

Host Resource Service 提供：

- `/css/user.css`
- `/scripts/extensions/third-party/*`
- `/thumbnail`
- `/characters/*`
- `/User Avatars/*`
- `/backgrounds/*`
- `/assets/*`
- `/user/images/*`
- `/user/files/*`

上游前端和第三方扩展可以继续把这些路径当作普通浏览器子资源 URL 使用。Host Resource Service 不改变 URL 形态、查询参数意义或角色数据事件语义。

## 2. 架构模型

实现区分四个概念：

```text
Resource       URL 指向的逻辑资源
Source         adapter 最终选择并打开的 backing file
Representation 浏览器实际观察到的 bytes + metadata
Delivery       当前 Tauri/Wry/IPC 入口能承载的响应形式
```

缓存职责也按层分离：filesystem 是资源事实，Rust thumbnail cache 是由 source revision 与 recipe 标识的派生产物，Host Resource Service 是 WebView 可见表示的新鲜度权威，WebView cache 只做机会性存储。mutation 不维护第二套 revision registry，也不依赖 WebView 是否真的保存 synthetic response 才能保证正确性。

`tt-ports::HostResourceAssetStore::open` 一次完成选源和打开文件，返回：

- MIME、长度、mtime、opaque source revision；
- 持有已打开 handle 的一次性 body reader。

这样 local/global extension 与 original/generated thumbnail 不会在 metadata 和 body read 之间重新选择。HEAD 或 304 直接丢弃 body reader，不读取正文；原子替换路径后，已打开 handle 仍对应此前生成的 metadata。正文读取以 open 时长度为硬上界，并在读取后复核 handle 的长度与 mtime；共享数据目录中的外部原地写入或 Android replace fallback 若与读取并发，会 fail-fast，而不会组合旧 validator 与新正文。

HTTP request/response 直接使用 `http` crate 类型。旧的字符串 status/header DTO 已删除；Tauri presentation 只负责 origin gate、delivery capability 和 response header 合并。

## 3. 成功与错误缓存策略

当前所有未版本化 Host Resource 成功响应使用：

```http
Cache-Control: private, no-cache
ETag: W/"..."
Date: ...
Last-Modified: ...   # 表示具有可靠文件 mtime 时
```

`no-cache` 允许 WebView 存储响应体，但要求复用前验证。它不是 `no-store`。

以下响应继续使用 `Cache-Control: no-store`：

- OPTIONS；
- 4xx/5xx；
- 405；
- 416。

当前不为 data-root-backed mutable Host Resource 提供 immutable policy。它们允许外部与 symlink writer 修改；fresh immutable hit 会直接绕过 Rust，使 Host Resource 无法发现变化。`immutable` 只适合未来真正的构建期内容哈希资源或内容寻址存储，不属于当前 Host Resource 契约。

## 4. Validator 与条件请求

普通文件 validator 基于：

- source discriminator；
- 高精度 mtime；
- length；
- MIME；
- representation variant。

结果经过稳定哈希并标记为 weak ETag。Host Resource 不为 metadata 请求读取全文计算 strong digest。未来 mtime 会在 HTTP 表示中 clamp 到响应 Date；HTTP-date 无法表示的文件时间不发送 Last-Modified，仍由 ETag 完成验证。

请求按以下顺序处理：

1. 方法、路径、权限和文件存在性检查；
2. `If-None-Match`，使用 weak comparison；
3. 仅在没有 `If-None-Match` 时评估 `If-Modified-Since`；
4. HEAD；
5. Range 与 If-Range；
6. 最后读取 full/range body。

HEAD 在 service 最终出口统一清空正文，因此成功和错误状态都不会携带 body；Content-Length 仍描述对应 GET 表示。

支持 `If-None-Match: *` 和 tag list。Malformed optional conditional header 按 HTTP 语义忽略；现有 malformed/multi-range 继续返回 416。

当前文件 ETag 和 Last-Modified 不能证明 strong equality，因此有效 If-Range 不会命中：服务忽略 Range 并返回完整 200，避免把旧 partial representation 与新正文拼接。

304 响应携带 Date、ETag、Cache-Control 和适用的 Last-Modified，不携带 body、Content-Type、Content-Length 或 Content-Range。

## 5. 各资源表示

| 资源 | 表示身份 | Last-Modified | HEAD Content-Length |
| --- | --- | --- | --- |
| user.css | backing file revision + 最终 CSS MIME | source mtime | 精确 |
| user-data | kind + backing file revision | source mtime | 精确 |
| third-party raw | local/global + backing file revision | source mtime | 精确 |
| `ttCompat=layer` | source revision + transform revision | 不发送 | 省略 |
| thumbnail original | kind + source file revision | source mtime | 精确 |
| thumbnail generated JPEG | source revision + generator recipe + cached file revision | cached file mtime | 精确 |

`ttCompat=layer` 在请求时转换正文，未读取正文前无法知道最终长度，因此 HEAD 合法省略 Content-Length，不使用源 CSS 的错误长度。

Thumbnail 继续复用现有磁盘缓存，但缓存命中不再依赖 JPEG mtime 恰好等于 source mtime。生成 JPEG 的标准 COM marker 内保存 semantic identity，内容绑定：

- source kind、length、高精度 mtime 与 MIME 形成的 source revision；
- width、height、quality、resize mode；
- JPEG/filter/generator schema version。

identity 与 JPEG 是同一个原子 artifact：旧缓存、上游 SillyTavern 或外部进程生成的 JPEG 没有匹配 COM marker，都会自然 miss，不需要迁移分支，也不会把外部 bytes 误认成本 recipe 的产物。编码在锁外并行；单个 JPEG 的短提交/校验打开区串行化。生成读取以 source open 时长度为上界，并在读取后复核 handle length/mtime；atomic path replacement 继续对应旧 opened snapshot，原地变化则 fail-fast。generated Host revision 使用同一个 identity，因此 source 或 recipe 任一变化都会同时使磁盘产物与 ETag 失效。

业务 mutation 不删除或预热 thumbnail cache。原文件始终先于 cache 被打开；删除后的旧 artifact 不可达，同名重建或覆盖后的 demand 会因 COM identity 不匹配而原地替换。这样 background、character、persona、archive import、同步与外部/symlink writer 都遵循同一 freshness 条件，缓存维护失败也不会把已成功的业务 mutation 伪装成失败。

默认 `/thumbnail` 保持上游选择语义：静态图优先 generated JPEG，动画与无法生成的普通缩略图可回退 original。`/thumbnail?type=bg&...&static=true` 是 gallery/folder 专用的强制静态表示：可解码 GIF/WebP/APNG 生成 first-frame JPEG；生成失败直接报错，绝不把动画 original 冒充静态预览。MP4 不引入视频解码依赖，前端预览使用占位；active background 与媒体播放始终读取 raw `/backgrounds/*`。

## 6. 平台 delivery

Android `WebResourceResponse` 和当前 Wry Android adapter 不支持 300-399。Host Resource 因此先产生标准逻辑决策，再按入口交付：

| 入口 | validator 命中 |
| --- | --- |
| production Wry，非 Android | 304 |
| production Wry，Android | 完整 200 |
| dev `tt-ext` Wry，非 Android | 304 |
| dev `tt-ext` Wry，Android | 完整 200 |
| dev IPC -> JavaScript Response，非 Android | 304 |
| dev IPC -> JavaScript Response，Android | 完整 200 |

不得用 204、空 200 或 Wry patch 模拟 Android 304。

Android background video 的现有二次 Range workaround 保持：非零 Range 返回 206 和对应 range headers，但提供完整文件 stream，让 WebView 自己应用 Range。

## 7. Origin 与开发态

Production `on_web_resource_request` 只接收 canonical Rust-side URI：

```text
tauri://localhost/...
```

presentation 在路由前精确校验 scheme 和完整 authority；relative URI、外部 HTTP/HTTPS、其他 host 和带端口 authority 均不进入 Host Resource Service。

开发态 `tt-ext` 是独立注册的 trusted scheme。Production `tauri` protocol 与 dev `tt-ext` 都由 Wry 交付，因此共享同一个 delivery capability。直接 `fetch(tt-ext)` 时保留 `Request.cache` 和 `Range`；所有入口都保留 Host Resource 使用的 `Range`、`If-Range`、`If-None-Match`、`If-Modified-Since` 请求语义。`tt-ext` 对跨源 fetch 显式暴露响应头，避免 ETag、Range metadata 和 trace header 被 CORS 过滤。

跨源 custom scheme fetch 无法可靠承载会触发 CORS preflight 的条件头，因此 `If-Range`、`If-None-Match`、`If-Modified-Since` 请求直接走 IPC；无条件请求和单一 `Range` 继续走 `tt-ext` Wry。WebKit 的 Service Worker 上下文无法直接 fetch custom scheme 时，client bridge 改由 window 上下文 fetch 同一个 `tt-ext` endpoint，并用 transferable `ArrayBuffer` 回传正文；普通二进制资源不得降级为 serde JSON IPC。条件请求 IPC 通过通用 header wire 调用同一个 Host Resource Service，在非 Android 原样代理调用方显式条件请求的 304，在 Android 按 release 的平台约束返回完整 200；IPC 不启用仅属于 Wry Android 的二次 Range workaround。204/205/304 在 JavaScript `Response` 中统一使用 null body。当前契约不引入 Service Worker Cache API，也不声称 IPC 会自行生成浏览器缓存 validator；dev/release 共享的是资源表示与条件请求契约，自动缓存存储仍由各自 transport 负责。

## 8. 持续开发约束

- 不重新增加 `stat_xxx/read_xxx` 方法矩阵；
- 不在 presentation 或 route handler 手写字符串 status/header DTO；
- 新派生表示必须把变换 variant 纳入 ETag，且不能伪造未知 Content-Length；
- thumbnail recipe 或单文件缓存格式发生任何可观察变化时必须升级 generator schema；JPEG COM identity 与 generated Host revision 必须继续使用同一身份；
- data-root-backed mutable Host Resource 长期保持稳定 URL + `private, no-cache`；Host Resource Service 是 WebView 可见表示的新鲜度权威，这不是采用 immutable 前的临时 fallback；
- 不为 mutable Host Resource 建 mutation revision registry 或 `immutable` URL。data root 允许外部/symlink writer；fresh immutable hit 会绕过 Host Resource，令外部变化无法被发现；
- 第一方普通 browser resource 不得通过 Tauri asset protocol、base64 read command 或 server-thumbnail Blob cache 绕过本服务；
- mutation 只建立文件事实，既不维护/删除 Rust derived cache，也不预热 WebView cache；随后只负责触发稳定 URL 的真实 consumer demand，刷新不是 commit 的组成部分，也不得用随机 query 伪造 revision；
- frontend 仅保留有独立派生职责的 background preview cache；Host background 记录必须绑定当前 raw ETag 与 preview recipe；
- WebView synthetic response cache 只是优化，不能成为业务正确性的前提；
- Wry 升级必须重新审计 Android 3xx、header 和 cache-control 行为。
