# MCP M1 当前状态

## 当前解决的问题

M1 建立 MCP server 的本地稳定身份、严格持久化、显式启停、只读 tool discovery 与默认关闭的工具权限。它为后续 call/approval/Agent/Legacy 接入提供事实源，但本身不执行工具、不向模型公布工具。

## 端到端链路

```text
First-party MCP extension (React / strict TSX)
  -> window.__TAURITAVERN__.api.mcp
  -> presentation::mcp_commands
  -> tt-application::McpService
     -> McpServerRepository -> tt-adapter-storage-core
     -> McpGateway          -> tt-adapter-mcp -> shared HttpClientPool
  -> complete ToolCatalog + diagnostics
```

crate 责任：

- `tt-domain::models::mcp`：registration UUID、endpoint、Active/Paused、Off/Ask/Allow 与领域约束。
- `tt-ports::mcp` / `repositories::mcp_server_repository`：Tauri-free、RMCP-free 的两个 outbound port。
- `tt-application::McpService`：intent CRUD、permission、discovery 编排、canonical ToolId 与 stale setting 投影。
- `tt-adapter-storage-core`：`_tauritavern/mcp/registrations/<uuid>.json` 严格 v1 单文件存储与坏文件隔离。
- `tt-adapter-mcp`：RMCP 2026-first Auto lifecycle、一次受限的 2025 legacy lifecycle 尝试、bounded Streamable HTTP、手动全分页、schema/duplicate/size validation。
- `tt-adapter-http`：无 redirect、无 retry 的 MCP client profile；MCP adapter 每次 refresh 从共享 pool 取得当前 proxy/TLS/UA 配置。
- `tauritavern`：composition、commands 与 Manager Host ABI。

管理 UI 位于 `src/scripts/extensions/mcp-manager/`，由 SillyTavern 内置扩展加载器激活，并在 Extensions 抽屉中与 Skill 同级展示。React 组件只消费 typed initial state 与 actions；Host ABI 等待、MCP API 解析以及全部 SillyTavern Popup 交互（添加服务器表单、重命名输入、启用/移除确认）留在扩展 host adapter。TauriTavern Settings 不再拥有 MCP 入口。

## 长期不变量

1. canonical ID 沿用当前工具契约：provider 为 `mcp/<registration-uuid>`，ToolId 为 `mcp/<registration-uuid>:<native-name>`。
2. endpoint 不进入 ToolId，但 M1 中不可修改；换 endpoint 即新 registration、新权限。
3. discovery 只产生候选描述，不产生执行 authority；新工具永远 Off。
4. annotations 是不可信提示，不能改变 permission。
5. 完整分页成功后才发布 catalog；不发布 partial、不用旧 catalog 掩盖失败。
6. 坏 registration 隔离到单文件，坏 tool/duplicate 隔离到最小工具组，系统 IO 与分页不完整显式失败。
7. RMCP/reqwest 类型不得进入 domain、ports、application 或 presentation DTO。
8. MCP 不进入全局 Legacy ToolManager，M1 不修改 Agent/Legacy 生成语义。

## Cache 语义

M1 不维护 application-owned catalog cache。一次 refresh 创建一个 RMCP Peer，SDK cache 仅在该 Peer 内按协议提示工作，refresh 完成后随 Peer 销毁。因此失败会显式返回，Manager 只在当前页面内保留最近一次成功的展示数据；reload 后消失。

只有实际测量证明跨 refresh/重启 discovery 成为瓶颈，或 invocation 需要明确定义的离线 stale 语义时，才重新评估 semantic catalog cache。

## 当前固定边界

- HTTP JSON response：4 MiB；POST SSE discovery response：4 MiB 总量；GET SSE：4 MiB/event。
- transport 支持 HTTPS；HTTP 仅允许本机、明确的内网地址及本地命名空间，公网 HTTP 被拒绝。Manager 激活 HTTP registration 时会明确提示流量未加密。
- 单 tool wire representation：256 KiB；完整 catalog：8 MiB。
- 每个 server：最多 32 页、512 tools。
- HTTP connect/request timeout：30s / 60s；lifecycle 与分页各有 120s 总 timeout。
- redirect、SSE reconnect、expired-session reinitialize 与 application retry 均关闭。
- 仅当 2026-first Auto 启动返回 implementation-defined `-32000`，或 discovery 响应通道在 SDK 完成错误分类前关闭时，才在同一 lifecycle timeout 内用新 Peer 尝试一次 2025 initialize；其他 transport、auth、timeout 与 list 错误不触发该路径。

这些是代码常量，不是 per-server 设置。超限不会静默截断：单 tool 超限产生 tool diagnostic，无法确认完整分页或 catalog 总量超限则本次 server discovery 失败。

## 当前不支持

- tool call、审批、取消结果与 unknown-outcome；
- Agent/Legacy exposure 与 model alias；
- OAuth/credential、stdio、Resources/Prompts/Tasks/Apps；
- background refresh、list-changed、persistent cache、endpoint migration、sync dataset。

全量 Data Archive 会随 data root 备份 registration；TT-Sync 不自动传播 MCP trust/permission。
