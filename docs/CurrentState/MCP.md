# MCP M2 当前状态

## 当前解决的问题

M2 在 M1 的稳定 registration、严格持久化、显式启停、只读 discovery 与默认关闭权限之上，增加第一方 Manager 的用户 test call。用户在统一测试控制台中选择服务器与工具、按 input schema 生成的友好表单填写参数（复杂字段回退原始 JSON），真实执行一次工具并区分已知响应、可证明未发送与结果未知；系统仍不向模型公布 MCP 工具。

## 端到端链路

```text
First-party MCP extension (React / strict TSX)
  -> window.__TAURITAVERN__.api.mcp
  -> presentation::mcp_commands
  -> tt-application::McpService
     -> McpServerRepository -> tt-adapter-storage-core
     -> McpGateway          -> tt-adapter-mcp -> shared HttpClientPool
  -> complete ToolCatalog / bounded test-call outcome
```

crate 责任：

- `tt-domain::models::mcp`：registration UUID、endpoint、Active/Paused、Off/Ask/Allow 与领域约束。
- `tt-ports::mcp` / `repositories::mcp_server_repository`：Tauri-free、RMCP-free 的 outbound ports 与 MCP call outcome。
- `tt-application::McpService`：intent CRUD、permission、discovery、test-call Active/JSON gate、取消注册与 Host DTO 投影。
- `tt-adapter-storage-core`：`_tauritavern/mcp/registrations/<uuid>.json` 严格 v1 单文件存储与坏文件隔离。
- `tt-adapter-mcp`：RMCP 2026-first Auto lifecycle、一次受限的 2025 legacy lifecycle 尝试、bounded/cancellable Streamable HTTP、手动全分页、discovery validation 与单次 `tools/call` 结果投影。
- `tt-adapter-http`：无 redirect、无 retry 的 MCP client profile；MCP adapter 每次 discovery/call 从共享 pool 取得当前 proxy/TLS/UA 配置。
- `tauritavern`：composition、commands 与 Manager Host ABI。

管理 UI 位于 `src/scripts/extensions/mcp-manager/`，由 SillyTavern 内置扩展加载器激活，并在 Extensions 抽屉中与 Skill 同级展示。React 组件只消费 typed state 与 actions；Host ABI 等待、MCP API 解析以及全部 SillyTavern Popup 交互留在扩展 host adapter。测试调用收敛为工具栏单一入口的测试控制台弹窗：选择 active 服务器后自动 discovery、选择工具、按 schema 表单填写并查看当次结果；没有历史/回放/自动重试。TauriTavern Settings 不再拥有平行入口。

## 长期不变量

1. canonical ID 沿用当前工具契约：provider 为 `mcp/<registration-uuid>`，ToolId 为 `mcp/<registration-uuid>:<native-name>`。
2. endpoint 不进入 ToolId，但当前不可修改；换 endpoint 即新 registration、新权限。
3. discovery 只产生候选描述，不产生执行 authority；新工具永远 Off。
4. annotations 是不可信提示，不能改变 permission。
5. 完整分页成功后才发布 catalog；不发布 partial、不用旧 catalog 掩盖失败。
6. 坏 registration 隔离到单文件，坏 tool/duplicate 隔离到最小工具组，系统 IO 与分页不完整显式失败。
7. user test call 的一次点击就是本次 authority；Off/Ask/Allow 不阻止调用，也不被调用修改。
8. request handle 建立前的失败才可标记 NotSent；建立后 cancel/timeout/disconnect 无法证明远端状态时必须标记 OutcomeUnknown。
9. arguments 保持原始 JSON 字符串穿越 JavaScript Host 边界，并在 backend 解析；`i64/u64` 范围内的 JavaScript 不安全整数不会被前端数值转换破坏。
10. RMCP/reqwest 类型不得进入 domain、ports、application 或 presentation DTO。
11. MCP 不进入全局 Legacy ToolManager，M2 不修改 Agent/Legacy 生成语义。

## Peer 与 cache 语义

M2 不维护 application-owned catalog cache。一次 discovery 创建一个 RMCP Peer，SDK cache 仅在该 Peer 内按协议提示工作，完成后随 Peer 销毁。因此失败会显式返回，Manager 只在当前页面内保留最近一次成功的展示数据；reload 后消失。

每次 test call 也使用新的短生命周期 Peer。协商到 `2026-07-28` 时，RMCP 3.1.2 构造 SEP-2243 `Mcp-Param-*` 需要同一 transport worker 的 tool schema cache，因此 call 前在同一 Peer 分页 `tools/list`，找到目标工具后立即停止；目标始终未出现时才遍历完整目录。该步骤只 hydration transport metadata：不做 arguments schema validation/coercion，也不持久化 catalog；目标未出现在 SDK 可见列表时，明确 NotSent。2025 协议不做这次额外 list。

只有实际测量证明跨 refresh/重启 discovery 成为瓶颈，或 invocation 需要明确定义的离线 stale 语义时，才重新评估 semantic catalog cache。

## 当前固定边界

- HTTP JSON response：4 MiB；POST SSE discovery response：4 MiB 总量；GET SSE：4 MiB/event。
- test-call arguments JSON：256 KiB，且必须为 object；完整 call response wire 上限：4 MiB。
- transport 支持 HTTPS；HTTP 仅允许本机、明确的内网地址及本地命名空间，公网 HTTP 被拒绝。Manager 激活 HTTP registration 时会明确提示流量未加密。
- 单 tool wire representation：256 KiB；完整 catalog：8 MiB。
- 每个 server：最多 32 页、512 tools。
- HTTP connect/request timeout：30s / 60s；lifecycle 与分页各有 120s 总 timeout。
- redirect、SSE reconnect、expired-session reinitialize 与 application retry 均关闭。
- 仅当 2026-first Auto 启动返回 implementation-defined `-32000`，或 discovery 响应通道在 SDK 完成错误分类前关闭时，才在同一 lifecycle timeout 内用新 Peer 尝试一次 2025 initialize；其他 transport、auth、timeout 与 list 错误不触发该路径。

这些是代码常量，不是 per-server 设置。超限不会静默截断：单 tool 超限产生 discovery diagnostic，无法确认完整分页或 catalog 总量超限则本次 server discovery 失败；test call 在找到目标前的 metadata 分页超限为 NotSent，server 已响应后无法显示的内容保留 KnownResponse 并产生 diagnostic。无效的可选 output schema 只移除该字段并产生 diagnostic，不隔离仍可调用的工具。

## 当前不支持

- Agent/Legacy tool call 与 Ask 审批；
- Agent/Legacy exposure 与 model alias；
- OAuth/credential、stdio、Resources/Prompts/Tasks/Apps；
- background refresh、list-changed、persistent cache、endpoint migration、sync dataset。

全量 Data Archive 会随 data root 备份 registration；TT-Sync 不自动传播 MCP trust/permission。
