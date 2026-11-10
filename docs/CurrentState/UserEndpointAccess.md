# Custom Endpoint and Import Source Access

## 边界

`custom_url` 与 `reverse_proxy` 是必要的 SillyTavern 兼容能力，也是 WebView 中第三方扩展能够控制的网络出口。TauriTavern 对每个用户配置端点要求一次 Rust 原生 dialog 授权；公网、局域网、loopback、LAN hostname 与代理软件 Fake IP 使用同一规则。内置 provider 端点不需要授权。

本机制不识别扩展身份。扩展可以触发 dialog，但不能替用户确认；用户授权后，同一 WebView 内的代码都能使用该端点。更细的 WebView/native capability 边界另行设计。

## 授权与传输

- 复用 `parse_user_http_endpoint` 规范化 URL，并拒绝非 HTTP(S)、userinfo、query 与 fragment。
- grant 保存规范化端点字符串，精确包含 scheme、host、port 与 base path。
- 授权在 Rust command（chat completion 与 SearXNG 搜索）中检查，兼容路由和扩展请求无需单独预检。
- HTTP pool 对每次用户端点请求独立检查 grant，绕过 command gate 的调用会 fail closed。
- 显式 loopback、RFC1918、IPv6 ULA 地址及 `localhost` 强制 Direct；其他端点遵循 Request Proxy 设置。
- DNS 结果不参与授权或传输分类。LAN hostname 和 Fake IP 因此不会被解析地址误判。
- 重定向最多 5 次且必须同源。

原生 dialog 的安全文案和规范化端点由 Rust host 组装；WebView 只提供 locale，不提供实际展示文案或确认结果。用户取消是正常取消状态，不会被包装成连接错误。

## 持久化

grant 是一个 JSON 字符串数组，位置为：

```text
<app_root>/security/user-endpoint-grants.json
```

它不随 `data_root`、数据备份或导入迁移。文件缺失或不可读时按空 grant 启动。原生确认会立即授权当前会话，然后原子持久化；写入失败只记录 warning，本次连接继续，重启后需要重新确认。

旧的 `local-endpoint-grants.json` 不读取也不迁移。

iOS 的系统 Local Network permission 与本 grant 相互独立。Android 升到要求 `ACCESS_LOCAL_NETWORK` 的 target SDK 时，还需单独接入平台权限。

## 导入来源

导入下载（URL 导入角色卡、资源库下载、Skill 的 URL 导入）使用同一种授权，单位是来源（scheme、host、port）。`https://cdn.discordapp.com`、`https://files.catbox.moe`、`https://raw.githubusercontent.com`、`https://botbooru.com` 预先批准，其余来源首次使用时询问。

- 下载自己跟随重定向，首个请求与每个重定向目标按同一规则检查。
- grant 保存在 `<app_root>/security/import-source-grants.json`，与端点 grant 分开存放，处理方式相同。
- 保存图片（`stage_file_from_url`）不是导入，不经过此授权。
