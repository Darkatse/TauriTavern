# LLM Connection API

`api.llmConnections` 管理 Agent 可以引用的模型连接。连接保存 provider、端点、凭据引用和路由，Profile 另外选择模型 ID。

## 入口与方法

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const connections = window.__TAURITAVERN__.api.llmConnections;
```

| 方法 | 返回值 |
| --- | --- |
| `list()` | `{ connections }`，连接摘要列表 |
| `load(connectionId)` | `{ connection }`，不存在时为 `null` |
| `save(connection)` | 保存完成后返回 |
| `delete(connectionId)` | 删除完成后返回 |

`load`、`delete` 也接受 `{ connectionId }`，`save` 也接受 `{ connection }`。

## 连接内容

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
    secretRef?: { key: string; id: string; labelSnapshot?: string };
  };
  routing?: {
    reverseProxy?: { url: string } | { preset: string };
  };
  adapterHints?: Record<string, string>;
  capabilities?: Record<string, string>;
};
```

`id` 使用小写字母、数字、`-` 或 `_`，最长 128 字符。直接连接通过 `auth.secretRef` 引用密钥；使用反向代理时可以保留 `auth: {}`。

`routing.reverseProxy.preset` 引用用户保存的反代预设。解析连接时读取其 URL 与密码，预设更新作用于后续解析；模型名按 Profile 指定的值传递。

## Profile 与 Model Target

Profile 使用 `model.mode = "connectionRef"`，保存 `connectionRef` 和 `modelId`。具体配置见 [Profile 与预设](../Agent/ProfilesAndPreset.md)。

Agent System 将 Connection Manager 的 Model Target 物化为 `model-target-<target.id>` 连接，在启动、Target 更新、Profile 保存和 Run 启动时同步。无法表示的 Target 配置会报告错误，并移除对应旧连接，供 Profile 诊断显示。

连接同步会更新 provider、端点、凭据和 adapter 选项。`modelId` 属于 Profile，修改 Target 的模型名后，需要在 Profile 中重新选择才会采用新值。删除 Target 会保留已经物化的连接。

已经准备好的 Invocation 继续使用其模型请求，连接更新供后续解析使用。独立模型绑定导出时会移除本机引用，导入后重新配置。

## 实现

- [llm_connection.rs](../../src-tauri/crates/tt-domain/src/models/llm_connection.rs)：连接结构。
- [llm_connection_service.rs](../../src-tauri/crates/tt-application/src/services/llm_connection_service.rs)：解析与请求字段应用。
- [model-target-llm-connection.js](../../src/scripts/tauritavern/agent/model-target-llm-connection.js)：Model Target 同步。
