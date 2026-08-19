# Custom Endpoint Non-Public Access

## 边界

`custom_url` 与 `reverse_proxy` 是必要的 SillyTavern 兼容能力，但第三方扩展也能驱动同一 WebView。TauriTavern 因此默认只允许公网目的地址；端点仅解析到非公网地址时，需要用户在 Connect 触发的 Rust 原生 dialog 中确认一次。该规则同时覆盖 loopback、LAN、link-local、CGNAT、代理软件 Fake IP 和其他非全局可达地址，不再维护额外的永久禁用类别。

本机制不识别扩展身份。扩展可以请求显示 dialog，但不能替用户确认；一旦用户授权，同一 WebView 内的代码都能使用该端点。更细的 WebView/native capability 边界另行设计。

## 最小授权模型

- 复用 `parse_user_http_endpoint` 规范化 URL，并拒绝非 HTTP(S)、userinfo、query 与 fragment。
- grant 只保存规范化端点字符串，精确包含 scheme、host、port 与 base path。
- 未授权端点使用只接受公网地址的 resolver，并尊重 Request Proxy；若 DNS 同时返回公网与非公网地址，只连接公网地址且不请求授权。
- 已授权的显式 loopback、RFC1918、IPv6 ULA 地址与 `localhost` 强制 Direct；其他端点继续尊重 Request Proxy。两种 trusted route 都允许连接时解析到公网或非公网地址。
- hostname 每次连接重新解析；地址变化无需重写 grant，连接时 resolver 仍执行对应的 trusted/public 策略。
- 重定向最多 5 次且必须同源。

原生 dialog 的安全文案、规范化端点和解析地址均由 Rust host 组装；WebView 只提供 locale 提示，不能提供实际展示文案或替代确认结果。

Connect 只负责取得授权；status/generate 会在 HTTP pool 中独立执行同一 grant 与地址策略，不能靠伪造前端结果绕过。自动 reconnect 只检查 grant，未授权时保持未连接并给出非错误提示，不显示 dialog；绕过前端的请求仍会被 HTTP 层拒绝。

## 持久化

grant 是一个 JSON 字符串数组，位置为：

```text
<app_root>/security/local-endpoint-grants.json
```

它不随 `data_root`、数据备份或导入迁移。文件缺失或不可读时按空 grant 启动，公网连接不受影响。原生确认会立即授权当前会话，然后原子持久化；写入失败只记录 warning，本次连接继续，重启后需要重新确认。

iOS 的系统 Local Network permission 与本 grant 相互独立。Android 升到要求 `ACCESS_LOCAL_NETWORK` 的 target SDK 时，还需单独接入平台权限。
