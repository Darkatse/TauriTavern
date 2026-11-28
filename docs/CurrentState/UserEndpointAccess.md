# Custom Endpoint and Import Source Access

## 边界

用户端点是目的地由页面或页面可写的配置决定的原生出站请求，并且请求会带上宿主密钥、把响应交回页面，或者发出 WebView 自己发不了的请求，例如 `custom_url`、`reverse_proxy`、CORS 代理 `/proxy/<URL>`、自托管服务地址与 MCP 服务器。每个来源由用户在[原生对话框](../FrontendGuide.md#22-原生能力)里授权一次，公网、局域网、loopback、LAN hostname 与代理软件的 Fake IP 适用同一规则。授权不区分扩展，授权后同一 WebView 里的代码都能使用该端点。

内置 provider 的地址不需要授权，包括自托管服务未配置时的默认地址。导入下载使用单独的[来源授权](#导入来源)。扩展 Git 远程只发 smart HTTP GET，内容不回到页面；保存图片的下载留在宿主。这两类都不是用户端点。

## 内置主机

内置 provider 的主机由宿主写定，页面的值只能在其中选择：

- 拼进主机名的值（region、作为子域的模型名等）先解析为 `HostLabel`：1 到 63 个 ASCII 字母、数字或连字符，首尾不是连字符。
- 只有几个固定主机的 provider，按枚举选择主机。

不合法的值返回校验错误，不发出请求。

## 授权与传输

- 授权单位是来源（scheme、host、port），写法与浏览器相同，例如 `https://example.com:8443`，只接受 HTTP(S)。授权覆盖该来源下的任意路径与查询串。
- 各入口自己校验输入：chat completion、文本补全与 SearXNG 的基址拒绝 userinfo、query 与 fragment；CORS 代理的目标可以是该来源下的任意地址。
- 命令在发送前询问。不经过命令的请求（Agent 的模型请求与 MCP 工具调用）不询问。
- 请求经 `HttpClientPool::user_endpoint_client` 发送，pool 按每次请求的来源检查授权，未授权即拒绝。
- 显式 loopback、RFC1918、IPv6 ULA 地址与 `localhost` 直连，其余遵循 Request Proxy 设置。授权与直连都不看 DNS 解析结果。
- 重定向最多 5 次且必须同源；本身不跟随重定向的请求（MCP）仍不跟随。

## 持久化

授权保存在 `<app_root>/security/user-endpoint-grants.json`，内容是来源组成的 JSON 字符串数组，不随数据目录、备份或导入迁移。文件缺失或不可读时按空列表启动，加载时每一项换算成来源并去重。确认后当前会话立即生效，再原子写入；写入失败只记录 warning，重启后需要重新确认。

系统的本地网络权限与这里的授权相互独立，Android 的要求见 [AndroidDevelopment](../AndroidDevelopment.md)。

## 导入来源

URL 导入、资源库下载与 Skill 的 URL 导入同样按来源授权。常用文件托管与已识别站点预先批准，其余来源首次使用时询问。下载自己跟随重定向，每一跳按同一规则检查。授权保存在 `<app_root>/security/import-source-grants.json`，处理方式与端点授权相同。

## 修改入口

- 新增入口：服务给出目的地（`resolve_user_endpoint` 或构造好的请求），命令调用 `ensure_user_endpoint_access` 询问，adapter 经 `user_endpoint_client` 发送。
- 来源的算法：`tt-domain` 的 `endpoint_url::http_origin`；授权列表：`tt-application` 的 `user_grants.rs`；路由、直连与重定向：`tt-adapter-http` 的 `restricted_endpoint.rs`。
- 内置主机：`endpoint_url::HostLabel`；Vertex AI 的 region 与主机：`tt-domain` 的 `vertex_ai.rs`。
- 预先批准的导入来源：`external_import_service.rs` 的 `PRE_APPROVED_IMPORT_SOURCES`。
- 对话框：`presentation/commands/access_consent.rs`，文案在 `user_endpoint_access.rs` 与 `import_source.rs`。
