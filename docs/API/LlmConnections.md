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

## Connection Manager Model Target 生命周期

Agent System 负责把 Connection Manager 中的 chat-completion Model Target 同步为 `id = "model-target-" + target.id` 的 LLM Connection：

- Agent System 启动时会 reconcile 当前保存的 Model Target，并覆盖写入对应 LLM Connection。
- Model Target 创建或更新后，会立即重新物化对应 LLM Connection；更新 API key 时，新的 `secretRef.id` 因此会进入 Agent domain 的 connection definition。
- 启动 reconcile 或 Model Target 更新无法物化时，Agent System 会删除对应 `model-target-*` LLM Connection，让 Profile 诊断和运行按 missing connection fail-fast，而不是继续使用旧连接。
- Profile 保存前会重新读取当前 Model Target 列表，再按 `connectionRef + modelId` 找到对应 target 并物化，避免打开面板后的旧快照覆盖新 connection。
- Agent run 启动前会对当前 Profile 的 `model-target-*` binding 再执行一次同样的物化，确保 prompt assembly 与 runtime 看到 Connection Manager 中最新的 endpoint/provider/API key；该步骤按 `connectionRef` 找源 Model Target，不改写 Profile 的 `modelId`。
- 删除 Model Target 不会自动删除已经物化的 LLM Connection。Profile 是否继续可运行由 `connectionRef` 指向的 LLM Connection 是否存在决定，避免 UI 清理操作隐式破坏已有 Profile。
- `modelId` 属于 Profile binding，不属于 LLM Connection。更新 Model Target 的模型名不会静默改写已有 Profile；需要用户在 Profile 面板重新选择该 Model Target 才会采纳新的 `modelId`。

运行中 invocation 不热替换已解析的 model binding；新的 LLM Connection 会在后续 profile/model binding resolution 中生效。

当 Profile 被导出或嵌入到 Preset/Character 时，`connectionRef + modelId` 必须改写为：

```json
{
  "model": {
    "mode": "requiresConfiguration"
  }
}
```

`requiresConfiguration` 是合法可保存状态，但不可运行；用户需要在本机重新选择模型。

要求：

- 连接转换必须保真；无法表示的字段必须报错，不静默丢弃。
- `modelId` 属于 Profile binding，不属于 connection definition。
- Tauri command 名 `list_llm_connections` / `save_llm_connection` 等属于 Internal 实现细节，不是 Public Contract。
