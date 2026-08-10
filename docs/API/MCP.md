# `window.__TAURITAVERN__.api.mcp` — MCP Manager API

本文档描述已经落地的 MCP Manager Host ABI，以及 Agent 与 Legacy generation 的消费契约。MCP 是独立平台能力；当前提供 registration、persistent tool catalog、显式 refresh、第一方 Manager test call，并由 Agent 与 Legacy generation 消费 cached tools。

状态：Manager、Agent 与 Legacy generation 集成已实现，Project Contract（实验性）。

第一方管理 UI 位于 Extensions 抽屉的 MCP 内置扩展中。该扩展只是 `api.mcp` 的 React/strict TSX presentation；TauriTavern Settings 不再提供平行入口。

## 1. 入口

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const mcp = window.__TAURITAVERN__.api.mcp;
```

## 2. API

```ts
type TauriTavernMcpApi = {
  servers: {
    list(): Promise<{ servers: McpServer[]; storageIssues: McpStorageIssue[] }>;
    create(input: { displayName: string; endpoint: string }): Promise<McpServer>;
    rename(input: { registrationId: string; displayName: string }): Promise<McpServer>;
    setState(input: { registrationId: string; state: 'active' | 'paused' }): Promise<McpServer>;
    remove(input: string | { registrationId: string }): Promise<void>;
    discover(input: string | { registrationId: string }): Promise<McpDiscoveryResult>;
    refresh(input: string | { registrationId: string }): Promise<McpDiscoveryResult>;
  };
  tools: {
    setPermission(input: {
      registrationId: string;
      nativeName: string;
      permission: 'off' | 'ask' | 'allow';
    }): Promise<McpServer>;
    testCall(input: {
      registrationId: string;
      nativeName: string;
      argumentsJson: string;
    }, options?: { signal?: AbortSignal }): Promise<McpTestCallOutcome>;
  };
};
```

没有 `connect`、`disconnect` 或 `connected`。只有 cold discovery、显式 refresh 和 test call 才建立短生命周期 RMCP client；memory/disk catalog hit 不连接 server。`active` 表示允许读取该 registration 的 snapshot，并在需要时向其 endpoint 发起 discovery 或用户 test call。

## 3. DTO

```ts
type McpServer = {
  id: string;                    // canonical lowercase UUID
  displayName: string;
  endpoint: string;              // normalized, immutable
  state: 'active' | 'paused';
  toolPermissions: Record<string, 'ask' | 'allow'>;
};

type McpDiscoveryResult = {
  registrationId: string;
  protocolVersion: string;
  serverName?: string;
  serverVersion?: string;
  tools: Array<{
    id: string;                  // mcp/<registration-uuid>:<native-name>
    nativeName: string;
    title?: string;
    description?: string;
    inputSchema: object;
    outputSchema?: object;
    annotations: object;         // untrusted hints only
    permission: 'off' | 'ask' | 'allow';
  }>;
  diagnostics: Array<{
    code: string;
    nativeName?: string;
    message: string;
  }>;
  staleTools: Array<{
    nativeName: string;
    permission: 'ask' | 'allow';
  }>;
};

type McpTestCallOutcome =
  | {
      outcome: 'known_response';
      response:
        | {
            kind: 'tool_result';
            isError: boolean;
            textBlocks: Array<{ index: number; text: string }>;
            structuredJson?: string;
            diagnostics: Array<{ code: string; message: string; contentIndex?: number }>;
          }
        | { kind: 'server_error'; code: number; message: string; dataJson?: string }
        | { kind: 'unsupported_response'; responseType: string; message: string };
    }
  | { outcome: 'not_sent'; code: string; message: string }
  | { outcome: 'outcome_unknown'; code: string; message: string };
```

`storageIssues` 显式报告损坏、未知 schema/kind、非 canonical 文件名或文件内 ID 不匹配；健康 registration 仍正常返回。

## 4. Registration 契约

- `create()` 总是创建 `paused` registration；Manager 在切换为 Active 前展示并确认 exact endpoint。
- Manager 首次加载时会通过现有 `create()` 契约添加一次 Paused 的 Exa Search 推荐项；处理标记保存在扩展 store 中。删除该普通 registration 不会清除标记，因此同一 data root 中不会自动恢复。
- endpoint 是 registration 的信任身份事实，当前不提供修改方法。更换 endpoint 必须新建 UUID，工具权限重新从 Off 开始。
- display name 可以修改，不影响 UUID 或 ToolId。
- `off` 是缺省值，不写入 `toolPermissions`；`setPermission(..., 'off')` 删除对应持久设置。
- discovery 消失的 Ask/Allow 设置作为 `staleTools` 返回，但不会成为可用工具。
- registration 保存为 `_tauritavern/mcp/registrations/<uuid>.json` 的严格 v1 单文件记录；persistent catalog 保存为 `_tauritavern/mcp/catalogs/<uuid>.json` 的独立严格 v1 记录。两者都没有旧 schema reader 或 revision graph。

## 5. Discovery 契约

- transport 仅支持 unauthenticated Streamable HTTP。
- endpoint 支持 HTTPS。HTTP 只允许明确的本机/内网目标：localhost、单标签主机名、`.local` / `.home.arpa`，IPv4 loopback/private/link-local/shared address space，以及 IPv6 loopback/ULA/link-local；公网 HTTP 域名和地址仍被拒绝。userinfo、query、fragment 与 redirect 均被拒绝。
- 使用 RMCP 3.1.2 `ClientLifecycleMode::Auto` 优先协商 `2026-07-28`；标准 `-32022` 协商与 SDK 可见的 `-32601` legacy fallback 由 RMCP 处理。若 Auto 启动返回 implementation-defined `-32000`，或有限 SSE error 响应在 SDK 中退化为 `ConnectionClosed`，则用新 Peer 单次尝试 `2025-11-25` initialize；该额外路径不匹配其他错误。
- `tools/list` 必须完整分页；cursor 循环、页数/工具数/catalog 总量超限或分页失败会使该 server 的本次 discovery 失败，不返回 partial catalog。
- duplicate native name 隔离整个同名组；无效 input schema、单工具超限或名称无效只隔离该工具并返回 diagnostic。无效的可选 output schema 只移除该字段并返回 diagnostic，不阻止工具使用。
- input/output schema 按 JSON Schema 2020-12 编译验证；不会读取远端 `$ref`。
- annotations 只原样展示，不授予权限。
- `servers.discover()` 按 application memory、persistent file、真实 discovery 的顺序读取。磁盘 snapshot 绑定 registration UUID 与规范化 endpoint，载入后重新执行 application canonical validation。
- `servers.refresh()` 始终使用新的 RMCP Peer，绕过 memory/disk snapshot。cold discovery/refresh 在完整分页和全部 validation 成功后发布；写盘失败返回可用的 memory-only catalog 与 `mcp.catalog_persistence_failed` diagnostic，并保留旧磁盘 snapshot。网络或 validation 失败仍 reject，旧 snapshot 保留但不作为该请求的返回值。
- permission 与 `staleTools` 每次按当前 registration 投影，不写入 catalog snapshot。Paused gate 先于 snapshot lookup；registration 删除不受派生 catalog 清理失败影响，清理失败只记录 warning。
- catalog 跨应用重启保留，由用户手动 refresh 决定何时更新；没有 TTL、后台 refresh、list-changed subscription、自动 retry、source/age 字段或 migration reader。损坏/未知 schema/UUID/endpoint mismatch 显式报错，refresh 可绕过并修复。

## 6. User test call 契约

`tools.testCall()` 是第一方 Manager 的显式用户动作，不是面向任意扩展的通用 raw RPC：

- frontend 只提交 registration ID、native tool name 与原始 `argumentsJson`；endpoint、header、RMCP session 均由 backend registration 与 transport 决定。
- server 必须为 Active。Off/Ask/Allow 不阻止用户 test call，调用也不会修改保存的 permission。
- `argumentsJson` 上限为 256 KiB，backend 权威解析为 JSON object；不按 input schema 补默认值、转换类型、删除字段或在 server 之前做业务校验。字符串 Host 边界与 Rust `serde_json` 解析共同保留 `i64/u64` 范围内的 JavaScript 不安全整数。
- 每次点击只发送一次 `tools/call`，没有自动 retry。RMCP 2026 协商会在同一 Peer 上分页 `tools/list` 直到找到目标工具，仅用于填充 SEP-2243 standard-header metadata；目标始终未出现时才遍历完整目录。它不成为 application catalog cache，也不改变调用参数。
- 使用一次响应的低层 request handle，不自动驱动 `input_required`、Tasks 或其他 MRTR 后续轮次；这些 server 响应以 `unsupported_response` 明确展示。
- `AbortSignal` 表示停止本地等待，不能撤销远端副作用。Host 在调用前使用私有 start acknowledgement 建立取消事实，避免 cancel-before-register 竞态；该命令不属于公共 API。

顶层 outcome 是远端事实，而非 UI loading 状态：

- `known_response`：server 已明确响应。`isError: true` 仍是 tool result；JSON-RPC error 是 `server_error`，都不变成 command rejection。
- `not_sent`：backend 能证明目标 `tools/call` 尚未交给 transport，例如 Paused、JSON 不合法、metadata hydration 失败或发送前取消。
- `outcome_unknown`：request handle 已建立后发生 cancel、timeout、disconnect 或无法确认响应；调用可能已经执行，系统绝不自动重试。

已知 tool result 按原顺序投影 text、确定性 structured JSON、`isError` 与 diagnostic。当前不显示的 image/audio/resource block 与 metadata 会产生可见 diagnostic，不会抹掉“server 已响应”的事实。完整 response 已受 4 MiB wire 上限约束，text/structured 不再做第二次静默截断；raw response 超限或 malformed 时则是 `outcome_unknown`。取得已知响应后的 client close/join 失败只记录日志，不会改写 outcome。

该入口沿用当前 WebView trust model：同一 WebView 内的第一方/vendor extension script 被视为用户授权代码，backend 不声称能证明一次 command 源自物理点击或隔离 hostile extension。若 trust model 改变，应增加真实 command capability boundary，而不是在 DTO 中伪造 click flag。

## 7. Agent 消费契约

Agent 没有新增公共 raw-call API。Profile v3 以 `mcp/<registration-uuid>:<native-name>` 选择工具；每个 root、return-mode child 与 handoff invocation 都通过 `McpService` 读取 memory→disk persistent snapshot，绝不因启动 Agent 隐式 cold discovery。无 snapshot 或工具不在缓存时，工具从该 invocation 省略并留下 diagnostic；用户在 Manager 中 discovery/refresh 后，下一个 invocation 才观察新目录。

只有 Active 且 permission 为 Ask/Allow、input schema root 明确为 `type: "object"` 的工具可以进入 Agent binding。当前按用户要求不实现 Ask 审批 UI：Ask 与 Allow 都自动执行；Off 与 Paused 在广告前过滤，并在实际发送前重新读取 registration 复核。参数必须是 256 KiB 内 JSON object，Host 不按 schema 改写。alias 由 Agent snapshot 生成，不属于 Manager DTO 或 MCP identity。

已知结果投影为现有 `AgentToolResult`。序列化结果超过 Agent Profile 的 `tools.mcpResultInlineCharLimit`（默认 50,000）时，完整 JSON 保存在 run 的只读可见 `tool-results/`，模型得到原始 `content` 最多前 3,000 个 Unicode 字符的前缀预览、路径与分段读取指引。该值只属于 Agent invocation 投影，不进入共享 `McpService` 调用契约。`outcome_unknown` 不自动 retry，也不伪造 tool result；当前没有审批/未知结果交互状态机，因此终止当前 Agent run。

## 8. Legacy 消费契约

Legacy 集成没有扩展上面的公共 `api.mcp`。Legacy 使用第一方内部 commands 从 `McpService` 获取 Active + Ask/Allow 的 cached descriptors，并通过 permission-aware call 用例执行；第三方仍没有公共 raw MCP executor。

- catalog preparation 只读 memory/disk snapshot，cache miss 不联网；单 server diagnostic 不阻塞健康 MCP/local tools 或普通生成。
- 每个实际 Legacy root generation 冻结 descriptors；工具递归复用 descriptor snapshot，但每轮根据当前 local view 重新分配 alias 并重建 provider schema。执行只解析当前 round binding；MCP 不进入全局 ToolManager、slash commands 或 extension enumeration。
- local alias 优先；settings hook 后只接受仍唯一存在的初始 MCP alias。删除、改名、重复或失去 binding 的 alias 不按名称/schema猜 canonical ToolId。
- `custom_include_body` / `custom_exclude_body` 继续作为用户最终 upstream body 意图；Legacy MCP 不保护、拒绝或重注入 `tools` / `tool_choice`。
- 调用只提交 canonical ToolId、ToolManager 序列化的 `argumentsJson` 与独立 UUID `executionCallId`；不做 input-schema coercion。provider `tool_call_id` 只用于聊天消息关联。发送前重新检查 registration Active、Off 与 cancellation；Ask/Allow 当前都自动执行。
- Known error 和普通 NotSent 进入现有 error Tool message，让模型下一轮修正。`outcome_unknown` 不重试、不伪造结果、不部分提交 Tool turn，并终止当前 root/group generation。
- 保存、事件与递归继续使用 SillyTavern Legacy first-class tool-turn contract；旧 `extra.tool_invocations` 只读，不恢复新写。

## 9. 明确未支持

当前没有以下 API 或行为：

- Ask 审批、公共 raw-call API及 Legacy unknown-outcome 恢复交互；
- OAuth、credential、stdio、2024 HTTP+SSE；
- Resources、Prompts、Tasks、Apps、subscriptions/list-changed；
- background discovery、discovery/list 通用 retry、catalog TTL/revision history；
- endpoint migration、scope hierarchy、Manager-defined/global model alias。

model alias 属于短命 Agent/Legacy invocation binding，不属于 registration/discovery/test call。Legacy 集成不把 MCP tool 注册进全局 SillyTavern `ToolManager`。
