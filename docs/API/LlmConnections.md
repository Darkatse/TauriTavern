# `window.__TAURITAVERN__.api.llmConnections` — LLM Connection API

本 API 是前端/扩展侧管理 Agent 可引用 LLM 连接定义的 Host ABI。它只暴露稳定 DTO，不暴露 Rust repository、文件路径或 Tauri command 名。

## 入口

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);

const llmConnections = window.__TAURITAVERN__.api.llmConnections;
```

## 方法

```ts
type TauriTavernLlmConnectionsApi = {
  list(): Promise<{ connections: LlmConnectionSummary[] }>;
  load(input: string | { connectionId: string }): Promise<{ connection: LlmConnectionDefinition | null }>;
  save(input: LlmConnectionDefinition | { connection: LlmConnectionDefinition }): Promise<void>;
  delete(input: string | { connectionId: string }): Promise<void>;
};
```

## DTO

```ts
type LlmConnectionDefinition = {
  schemaVersion: 1;
  kind: 'tauritavern.llmConnection';
  id: string;
  displayName: string;
  description?: string;
  provider: {
    chatCompletionSource: string;
    customApiFormat?: string;
  };
  endpoint?: {
    baseUrl?: string;
    sourceSpecific?: Record<string, unknown>;
  };
  auth: {
    secretRef: {
      key: string;
      id: string;
      labelSnapshot?: string;
    };
  };
  routing?: {
    reverseProxy?: { url: string };
  };
  adapterHints?: Record<string, string>;
  capabilities?: Record<string, string>;
};
```

`id` 必须满足 Rust domain contract：非空、长度不超过 128，只能使用小写 ASCII、数字、`-`、`_`。

## 与 Agent Profile 的关系

Agent Profile 的持久化字段仍然是：

```json
{
  "model": {
    "mode": "connectionRef",
    "connectionRef": "model-target-...",
    "modelId": "..."
  }
}
```

Profile 不保存 Connection Manager 的 `modelTargetId`。Profile 面板可以把用户保存的 Model Target 物化为一个 LLM Connection，再把 Profile 指向 `connectionRef + modelId`。这样 runtime 只依赖 Agent domain 的 LLM Connection contract，Connection Manager 只是 UI 输入来源。

要求：

- 连接转换必须保真；无法表示的字段必须报错，不静默丢弃。
- `modelId` 属于 Profile binding，不属于 connection definition。
- Tauri command 名 `list_llm_connections` / `save_llm_connection` 等属于 Internal 实现细节，不是 Public Contract。
