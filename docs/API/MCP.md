# `window.__TAURITAVERN__.api.mcp` — MCP Manager API

本文档描述已经落地的 MCP M1 Host ABI。MCP 是独立平台能力；当前只提供 registration 与只读 tool discovery，尚未接入 Agent 或 Legacy 生成。

状态：M1 已实现，Project Contract（实验性）。

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
  };
  tools: {
    setPermission(input: {
      registrationId: string;
      nativeName: string;
      permission: 'off' | 'ask' | 'allow';
    }): Promise<McpServer>;
  };
};
```

没有 `connect`、`disconnect` 或 `connected`。M1 每次 discovery 都建立短生命周期 RMCP client，完成全分页后关闭；`active` 只表示允许向该 registration 的 endpoint 发起显式 discovery。

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
```

`storageIssues` 显式报告损坏、未知 schema/kind、非 canonical 文件名或文件内 ID 不匹配；健康 registration 仍正常返回。

## 4. Registration 契约

- `create()` 总是创建 `paused` registration；Manager 在切换为 Active 前展示并确认 exact endpoint。
- endpoint 是 registration 的信任身份事实，M1 不提供修改方法。更换 endpoint 必须新建 UUID，工具权限重新从 Off 开始。
- display name 可以修改，不影响 UUID 或 ToolId。
- `off` 是缺省值，不写入 `toolPermissions`；`setPermission(..., 'off')` 删除对应持久设置。
- discovery 消失的 Ask/Allow 设置作为 `staleTools` 返回，但不会成为可用工具。
- registration 保存为 `_tauritavern/mcp/registrations/<uuid>.json` 的严格 v1 单文件记录；没有旧 schema reader、revision 或持久 catalog cache。

## 5. Discovery 契约

- transport 仅支持 unauthenticated Streamable HTTP。
- endpoint 支持 HTTPS。HTTP 只允许明确的本机/内网目标：localhost、单标签主机名、`.local` / `.home.arpa`，IPv4 loopback/private/link-local/shared address space，以及 IPv6 loopback/ULA/link-local；公网 HTTP 域名和地址仍被拒绝。userinfo、query、fragment 与 redirect 均被拒绝。
- 使用 RMCP 3.1.2 `ClientLifecycleMode::Auto` 优先协商 `2026-07-28`；标准 `-32022` 协商与 SDK 可见的 `-32601` legacy fallback 由 RMCP 处理。若 Auto 启动返回 implementation-defined `-32000`，或有限 SSE error 响应在 SDK 中退化为 `ConnectionClosed`，则用新 Peer 单次尝试 `2025-11-25` initialize；该额外路径不匹配其他错误。
- `tools/list` 必须完整分页；cursor 循环、页数/工具数/catalog 总量超限或分页失败会使该 server 的本次 discovery 失败，不返回 partial catalog。
- duplicate native name 隔离整个同名组；无效 schema、单工具超限或名称无效只隔离该工具并返回 diagnostic。
- input/output schema 按 JSON Schema 2020-12 编译验证；不会读取远端 `$ref`。
- annotations 只原样展示，不授予权限。
- 每次 refresh 使用新的 RMCP Peer。SDK 自带 cache 保持默认语义，但其生命周期止于本次 refresh；应用层不维护 last-complete/stale catalog，也不会在失败时静默返回旧结果。

## 6. 明确未支持

M1 没有以下 API 或行为：

- `tools/call`、test call、审批与结果投影；
- Agent ToolSet 或 Legacy ToolManager/generation overlay；
- OAuth、credential、stdio、2024 HTTP+SSE；
- Resources、Prompts、Tasks、Apps、subscriptions/list-changed；
- background discovery、discovery/list 通用 retry、persistent catalog cache；
- endpoint migration、scope hierarchy、model alias。

model alias 属于未来 invocation `ToolBinding`，不属于 registration/discovery。M1 不把 MCP tool 注册进全局 SillyTavern `ToolManager`。
