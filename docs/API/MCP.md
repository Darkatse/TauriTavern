# `window.__TAURITAVERN__.api.mcp` — MCP API Draft

本文档是 MCP Host ABI 草案。MCP 是独立平台能力，Agent 只是它的消费者之一。

状态：规划中，尚未实现。

## 1. 入口

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);

const mcp = window.__TAURITAVERN__.api.mcp;
```

## 2. 方法概览

```ts
type TauriTavernMcpApi = {
  listServers(): Promise<McpServerSummary[]>;
  connectServer(input: McpConnectServerInput): Promise<McpServerStatus>;
  disconnectServer(serverId: string): Promise<void>;
  listTools(serverId: string): Promise<McpToolSummary[]>;
  callTool(input: McpCallToolInput): Promise<McpToolResult>;
  listResources(serverId: string): Promise<McpResourceSummary[]>;
  readResource(input: McpReadResourceInput): Promise<McpResourceContent>;
  listPrompts(serverId: string): Promise<McpPromptSummary[]>;
  getPrompt(input: McpGetPromptInput): Promise<McpPromptContent>;
};
```

## 3. Server

```ts
type McpServerSummary = {
  id: string;
  displayName: string;
  transport: 'stdio' | 'http' | 'sse';
  enabled: boolean;
  connected: boolean;
  capabilities: {
    tools?: boolean;
    resources?: boolean;
    prompts?: boolean;
    sampling?: boolean;
  };
};
```

`connectServer()` 不应接受任意 command 字符串作为扩展/Agent 可传入参数。stdio server 配置必须来自用户设置或 allowlist。

## 4. Tools

```ts
type McpToolSummary = {
  serverId: string;
  name: string;
  title?: string;
  description?: string;
  inputSchema?: unknown;
  annotations?: unknown;
  approvalRequired?: boolean;
};

type McpCallToolInput = {
  serverId: string;
  name: string;
  arguments?: unknown;
  approvalToken?: string;
};
```

语义：

- 危险工具默认需要审批。
- call result 可以被 Agent 映射为 `ToolResult`。
- call 必须有超时与取消策略。

## 5. Resources

```ts
type McpResourceSummary = {
  serverId: string;
  uri: string;
  name?: string;
  description?: string;
  mimeType?: string;
};

type McpReadResourceInput = {
  serverId: string;
  uri: string;
};
```

MCP resource 不应自动进入 prompt。宿主、用户、profile 或 preset 决定是否纳入 ContextFrame。

## 6. Prompts

```ts
type McpPromptSummary = {
  serverId: string;
  name: string;
  title?: string;
  description?: string;
  arguments?: unknown;
};

type McpGetPromptInput = {
  serverId: string;
  name: string;
  arguments?: unknown;
};
```

MCP prompt 可以成为 PromptComponent，但不能覆盖 TauriTavern preset policy。

## 7. Security Contract

禁止：

- Agent/Preset/角色卡/世界书直接写 MCP stdio command。
- 从远端 config 自动创建本地 stdio server。
- 初期启用 MCP Sampling 自动模型调用。
- 未经审批调用 destructive/high-cost tool。

要求：

- server/command/args 对用户可见。
- per-server capability allowlist。
- per-tool approval。
- MCP call 写入 Agent journal 或 MCP audit log。

## 8. Agent Integration

Agent 看到的是：

```text
mcp.<server>.<tool> -> ToolRegistry ToolSpec
mcp resource        -> WorkspaceResource / ContextFrame
mcp prompt          -> PromptComponent
```

Agent 不直接操作 MCP config。

