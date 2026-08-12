# MCP 当前状态

## 当前解决的问题

当前 MCP 平台以稳定 registration、严格持久化、显式启停、只读 discovery 与默认关闭权限为基础，提供第一方 Manager 的用户 test call，并将完整成功的 catalog 保存为 application-owned persistent snapshot。Rust Agent Runtime 与 Legacy generation 都从同一 snapshot 消费 MCP tools；Legacy generation 不会污染全局 ToolManager。

## 端到端链路

```text
First-party MCP extension (React / strict TSX)
  -> window.__TAURITAVERN__.api.mcp
  -> presentation::mcp_commands
  -> tt-application::McpService
     -> McpServerRepository -> tt-adapter-storage-core
     -> McpGateway          -> tt-adapter-mcp -> shared HttpClientPool
  -> complete ToolCatalog / bounded test-call outcome

Agent Profile v3
  -> AgentRuntimeService invocation preparation
  -> McpService memory/disk-only catalog resolution
  -> InvocationToolSnapshot / ToolRequestGate
  -> McpService permission recheck
  -> McpGateway::call_tool
  -> Agent journal + tool-results + next model turn

Legacy Generate root
  -> McpService memory/disk-only permitted catalog
  -> private generation descriptors + per-round aliases
  -> existing provider hook / streaming and non-streaming parsers
  -> McpService permission recheck + cancellable call
  -> existing Assistant/Tool rows + TOOL_CALLS events + recursive Generate
```

crate 责任：

- `tt-domain::models::mcp`：registration UUID、endpoint、协议版本偏好、Active/Paused、Off/Ask/Allow 与领域约束。
- `tt-ports::mcp` / `repositories::mcp_server_repository`：Tauri-free、RMCP-free 的 outbound ports 与 MCP call outcome。
- `tt-application::McpService`：intent CRUD、permission、persistent catalog policy、discovery、test-call Active/JSON gate，以及 Agent/Legacy 共用的 cached model-tool resolution 与发送前 permission/arguments gate。
- `tt-adapter-storage-core`：registration 与 endpoint-bound catalog snapshot 的严格 v1 单文件存储。
- `tt-adapter-mcp`：RMCP Auto/固定版本 lifecycle、一次受限的同版本 legacy lifecycle 尝试、bounded/cancellable Streamable HTTP、手动全分页、discovery validation 与单次 `tools/call` 结果投影。
- `tt-adapter-http`：无 redirect、无 retry 的 MCP client profile；MCP adapter 每次 discovery/call 从共享 pool 取得当前 proxy/TLS/UA 配置。
- `tauritavern`：composition、commands 与 Manager Host ABI。

管理 UI 位于 `src/scripts/extensions/mcp-manager/`，由 SillyTavern 内置扩展加载器激活，并在 Extensions 抽屉中与 Skill 同级展示。React 组件只消费 typed state 与 actions；Host ABI 等待、MCP API 解析以及全部 SillyTavern Popup 交互留在扩展 host adapter。Add 弹窗支持手动 header rows、协议版本选择，以及单服务器的直接对象/`mcpServers` JSON 输入；两者都只调用同一个 create 用例。已保存 server 的 Edit 弹窗复用手动连接表单，可修改名称、endpoint、headers 与协议版本。测试调用收敛为工具栏单一入口的测试控制台弹窗：选择 active 服务器后自动 discovery、选择工具、按 schema 表单填写并查看当次结果；没有历史/回放/自动重试。TauriTavern Settings 不再拥有平行入口。

## 长期不变量

1. canonical ID 沿用当前工具契约：provider 为 `mcp/<registration-uuid>`，ToolId 为 `mcp/<registration-uuid>:<native-name>`。
2. endpoint 不进入 ToolId；用户可以编辑 endpoint，registration UUID、ToolId 与权限保持不变，connection catalog 会失效。
3. discovery 只产生候选描述，不产生执行 authority；新工具永远 Off。
4. annotations 是不可信提示，不能改变 permission。
5. 完整分页与 application canonical validation 成功后才发布新 catalog；写盘失败时以可见 diagnostic 明确标记 memory-only，不发布协议 partial，其他 refresh 失败不把旧 snapshot 冒充本次结果。
6. 坏 registration 隔离到单文件，坏 tool/duplicate 隔离到最小工具组，系统 IO 与分页不完整显式失败。
7. user test call 的一次点击就是本次 authority；Off/Ask/Allow 不阻止调用，也不被调用修改。Agent 与 Legacy 模型调用中 Ask/Allow 均表示自动执行，审批系统留给后续统一工具交互设计。
8. request handle 建立前的失败才可标记 NotSent；建立后 cancel/timeout/disconnect 无法证明远端状态时必须标记 OutcomeUnknown。
9. Manager test call 将用户填写的 `argumentsJson` 原样交给 backend 解析；Legacy 沿用 ToolManager 的 provider 参数规范化与序列化结果。两条路径都不做 input-schema coercion。
10. RMCP/reqwest 类型不得进入 domain、ports、application 或 presentation DTO。
11. MCP 不进入全局 Legacy ToolManager、slash commands 或 extension enumeration；Legacy 只通过 request-scoped resolver 分派，local registry 保持 live lookup。
12. `custom_include_body` / `custom_exclude_body` 是用户最终 upstream body 意图；Legacy MCP 不保护、拒绝、重注入或改写 `tools` / `tool_choice`。
13. endpoint、custom headers 与协议版本作为同一份连接配置原子更新。所有 discovery/call 消费路径从 registration 读取该配置，list DTO 为同一 WebView 的编辑器回传 endpoint 与明文 headers。

## Persistent catalog 与 Peer 语义

`McpService` 按 registration 持有内存热副本，`tt-adapter-storage-core` 在 `_tauritavern/mcp/catalogs/<uuid>.json` 保存严格 v1 persistent snapshot。文件同时绑定 canonical UUID 与 registration 的规范化 endpoint；permissions 与 staleTools 不写入 catalog，而是在每次读取时根据当前 registration 投影。

普通 `servers.discover` 按内存、磁盘、真实 network discovery 的顺序读取。内存和磁盘均不存在时才创建 RMCP Peer；显式 `servers.refresh` 始终绕过两级 snapshot。cold discovery/refresh 在完整分页与 adapter/application validation 成功后发布；原子写盘失败不会阻止使用已验证 catalog，而是保留旧磁盘 snapshot、发布 memory-only snapshot，并返回 `mcp.catalog_persistence_failed` diagnostic。网络或 validation 失败仍直接返回错误，上一份 snapshot 保持不变但不会作为该请求的成功结果。损坏、未知 schema、UUID 或 endpoint 不匹配的文件显式失败，用户 refresh 可以直接联网并替换。

snapshot 没有 TTL/LRU、后台 refresh、自动 retry、source/age DTO 或 migration reader。registration 删除成功不受派生 catalog 清理失败影响；清理失败只写 warning。Data Archive/TT-Sync 的既有 external-data reconciliation 清空内存热副本，使后续读取重新观察当前 data root。仅改名称、Active/Paused 与 permission 不清 catalog；endpoint、headers 或协议版本变化先删除磁盘/内存 snapshot，再保存新连接配置。Paused gate 始终先于 cache lookup。

模型工具准备是严格 cached-only：共享 resolver 只处理 Active 且至少保存一个 Ask/Allow 的 registration，按 memory→disk 读取 snapshot，不调用 gateway、不做 cold discovery；没有 permission 的 registration 不触碰 catalog。单 registration cache miss、损坏 snapshot 或非 object-root function schema 只形成 diagnostic，健康 server 继续。Agent 再按 Profile 选择收窄；Legacy 每个实际 root generation 读取一次全量 permitted descriptors，并在工具递归中复用。用户在 Manager 显式 refresh 后，下一次 Agent invocation 或 Legacy root 才看到变化。

每次 test call 也使用新的短生命周期 Peer。协商到 `2026-07-28` 时，RMCP 3.1.2 构造 SEP-2243 `Mcp-Param-*` 需要同一 transport worker 的 tool schema cache，因此 call 前在同一 Peer 分页 `tools/list`，找到目标工具后立即停止；目标始终未出现时才遍历完整目录。该步骤只 hydration transport metadata：不做 arguments schema validation/coercion，也不持久化 catalog；目标未出现在 SDK 可见列表时，明确 NotSent。2025 协议不做这次额外 list。

## 当前固定边界

- HTTP JSON response：4 MiB；POST SSE discovery response：4 MiB 总量；GET SSE：4 MiB/event。
- test-call arguments JSON：256 KiB，且必须为 object；完整 call response wire 上限：4 MiB。
- Agent MCP arguments 同样最多 256 KiB 且必须为 object；可广告 schema 的 root 必须显式为 `type: "object"`。
- Legacy MCP arguments 同样最多 256 KiB 且必须为 object；结果以现有 bounded MCP outcome JSON 内联到 Tool message，不复用 Agent workspace externalization。
- Agent MCP 的 structured content、diagnostics 与可操作错误详情会转为模型可读 `content`，原始结构仍保存在完整 JSON audit。模型可见 `content` 超过当前 Profile 的 `tools.mcpResultInlineCharLimit`（默认 50,000）时，同一完整 `content` 的行可读 `.txt` 视图保存在 run 的只读可见 `tool-results/`；模型上下文接收最多前 3,000 个 Unicode 字符的前缀预览、两条路径与分段读取指引。可读视图换行超长物理行，JSON 保留精确原始结果。该值只影响 Agent invocation 投影，不改变共享 MCP call outcome；统计复用 domain `TextMetrics`，不引入 tokenizer 依赖。
- transport 接受任意带 host 的 HTTP(S) endpoint，包括公网 HTTP、userinfo 与 query。Manager 激活 HTTP registration 时会明确提示流量未加密。
- custom headers 不设 reserved-name、数量或总量限制；名称和值原样保存，无法被 HTTP/MCP transport 接受时返回正常调用错误。endpoint credentials 与认证 headers 明文保存在 registration 中。
- 单 tool wire representation：256 KiB；完整 catalog：8 MiB。
- 每个 server：最多 32 页、512 tools。
- HTTP connect timeout 为 30s；lifecycle 与分页各有 120s operation timeout，已发送 tool call 的响应等待为 60s。共享 client 不再叠加第二个 request timeout。
- redirect、SSE reconnect、expired-session reinitialize 与 application retry 均关闭。
- Auto 优先尝试 `2026-07-28`、`2025-11-25`、`2025-06-18`、`2025-03-26`；固定版本只允许所选版本。仅当启动返回 implementation-defined `-32000`，或 discovery 响应通道在 SDK 完成错误分类前关闭时，才在同一 lifecycle timeout 内用新 Peer 尝试一次相同配置的 initialize；其他 transport、auth、timeout 与 list 错误不触发该路径。

协议版本候选集合由代码限定；其余边界不是 per-server 设置。超限不会静默截断：单 tool 超限产生 discovery diagnostic，无法确认完整分页或 catalog 总量超限则本次 server discovery 失败；test call 在找到目标前的 metadata 分页超限为 NotSent，server 已响应后无法显示的内容保留 KnownResponse 并产生 diagnostic。无效的可选 output schema 只移除该字段并产生 diagnostic，不隔离仍可调用的工具。

## Agent 调用语义

- Profile v3 的 MCP identity 是 `mcp/<registration-uuid>:<native-name>`；v1/v2 的 builtin native names 在 application load 时一次性迁移为 `builtin:<native-name>` 并原子写回。
- 模型 alias 为 `mcp__<normalized server displayName>__<normalized nativeName>`，碰撞用确定性数字后缀；alias 只从 invocation binding 解码，执行使用原始 native name。
- 实际发送前重新读取 registration；不存在、Paused 或 Off 都返回可恢复的 NotSent tool error。Ask 与 Allow 当前行为相同。
- known `isError`、server error 与 unsupported response 作为可恢复 Agent tool result；MCP 不产生 `AgentToolEffect`。
- `outcome_unknown` 可能已经执行，绝不自动 retry，也不回填一个虚构结果。当前无用户决策 UI，因此以非 retryable error 终止 run；已有聊天 commit 时沿用 Agent 的 partial-success 终态。

## Legacy 调用语义

- MCP descriptors 只存在于私有 generation context；每个实际 group member 的模型/工具递归链是独立 root。递归复用 descriptor snapshot，但每轮重新求值 local tools、根据当前 local view 分配 MCP aliases 并创建 provider objects；执行只解析当前 round map，历史 alias 不是新请求 authority。
- local aliases 保持生态优先；MCP 使用 `mcp__<server>__<tool>` 与确定性数字后缀。`CHAT_COMPLETION_SETTINGS_READY` 后只保留初始 MCP alias 与 post-hook 唯一同名 entry 的交集；删除、改名或重复 alias 不猜 ToolId。
- finalizer 只描述 frontend post-hook view。后端随后照常应用用户 `custom_include_body` / `custom_exclude_body`；Legacy MCP 不保护或重新注入任何 body 字段。
- response alias 命中本轮 MCP binding 时，通过独立 UUID `executionCallId` 调用 Rust；provider `tool_call_id` 只用于 Assistant/Tool 消息关联。初始 MCP alias 若失去最终 binding，会形成 NotSent Tool error，不回落到 live local registry。
- Known error 与非用户 Stop 的 NotSent 保存为普通 error Tool result，同批后续 call 继续。`OutcomeUnknown` 或用户 Stop 立即终止剩余 calls；unknown 不伪造 Tool result、不部分保存当前批次、不自动 retry，并显示“可能已执行”的持久警告。
- 完整已知批次继续使用现有原子 writer：Assistant `tool_calls[]`、物理 `role=tool` rows、`TOOL_CALLS_PERFORMED`、render、`TOOL_CALLS_RENDERED`、save、recursive Generate。Stop 发生后可以保存已经完整确认的批次，但不会递归。
- MCP 从不注册到全局 ToolManager；local callback、formatter、stealth、slash commands 和 extension enumeration 保持现有行为。

## 当前不支持

- Ask 审批及跨工具统一审批交互；
- Legacy unknown-outcome pause/resume/retry workflow；
- OAuth/credential、stdio、Resources/Prompts/Tasks/Apps；
- background refresh、list-changed、catalog TTL/revision history、MCP 专属 sync dataset。

全量 Data Archive 会随 data root 备份 registration、其中的明文 custom header values 与 persistent catalog。TT-Sync 是否包含这些文件仍由用户选择的通用 DatasetPolicy 决定；MCP 不增加专属同步协议。
