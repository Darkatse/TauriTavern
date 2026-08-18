# Custom Endpoint Local Network Access

## 边界

`custom_url` 与 `reverse_proxy` 是必要的 SillyTavern 兼容能力，但第三方扩展也能驱动同一 WebView。TauriTavern 因此默认只允许公网目的地址；loopback、RFC1918、IPv6 ULA 和解析到这些范围的 LAN hostname，需要用户在 Connect 触发的 Rust 原生 dialog 中确认一次。

本机制不识别扩展身份。扩展可以请求显示 dialog，但不能替用户确认；一旦用户授权，同一 WebView 内的代码都能使用该端点。更细的 WebView/native capability 边界另行设计。

## 最小授权模型

- 复用 `parse_user_http_endpoint` 规范化 URL，并拒绝非 HTTP(S)、userinfo、query 与 fragment。
- grant 只保存规范化端点字符串，精确包含 scheme、host、port 与 base path。
- 公网端点使用全局地址 resolver，并尊重 Request Proxy。
- 已授权本地端点强制 Direct，并使用只接受 loopback、RFC1918 或 IPv6 ULA 的 resolver。
- LAN hostname 每次连接重新解析；地址变化无需重写 grant，但不能越出本地地址范围。
- link-local、metadata、CGNAT、multicast、reserved 等其他非全局地址不可授权。
- 重定向最多 5 次且必须同源。

Connect 只负责取得授权；status/generate 会在 HTTP pool 中独立执行同一 grant 与地址策略，不能靠伪造前端结果绕过。自动 reconnect 只检查 grant，未授权时保持未连接并给出非错误提示，不显示 dialog；绕过前端的请求仍会被 HTTP 层拒绝。

## 持久化

grant 是一个 JSON 字符串数组，位置为：

```text
<app_root>/security/local-endpoint-grants.json
```

它不随 `data_root`、数据备份或导入迁移。文件缺失或不可读时按空 grant 启动，公网连接不受影响。原生确认会立即授权当前会话，然后原子持久化；写入失败只记录 warning，本次连接继续，重启后需要重新确认。

iOS 的系统 Local Network permission 与本 grant 相互独立。Android 升到要求 `ACCESS_LOCAL_NETWORK` 的 target SDK 时，还需单独接入平台权限。
