# LLM Connection API

`api.llmConnections` 管理 Agent 可以引用的模型连接。连接保存 provider、端点、凭据引用和路由；由模型（Model Target）物化的连接还带有 `modelId`，绑定它的 Profile 跟随该值，其他连接的模型由 Profile 的 `modelId` 决定。

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
  modelId?: string; // 由模型物化的连接携带；绑定它的 Profile 使用该值
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

`routing.reverseProxy.preset` 引用用户保存的代理预设。解析连接时读取其 URL 与密码，预设更新作用于后续解析；模型名取连接的 `modelId`，连接未携带时取 Profile 的 `modelId`。

## Profile 与 Model Target

Profile 使用 `model.mode = "connectionRef"`，保存 `connectionRef` 和 `modelId`。解析时连接带有 `modelId` 则以连接为准，Profile 的 `modelId` 只记录选择时的模型。具体配置见 [Profile 与预设](../Agent/ProfilesAndPreset.md)。

Agent System 将 Connection Manager 的 Model Target 物化为 `model-target-<target.id>` 连接，在启动、Target 更新、Profile 保存和 Run 启动时同步。无法表示的 Target 配置会报告错误，并移除对应旧连接，供 Profile 诊断显示。启动同步还会删除找不到对应 Target 的 `model-target-*` 连接，例如 Agent System 未运行时被删除的模型留下的连接。

连接同步会更新 provider、端点、凭据、adapter 选项和模型。物化连接带有 `modelId`，绑定到它的 Profile 与 App assistant 都使用这个值，因此修改 Target 的模型名后无需重新选择。删除 Target 会一并删除它物化的连接，诊断与运行都会报告模型缺失。删除前，Connection Manager 列出绑定该模型的 Agent Profile（含共享的 Session Profile），在确认框中提示它们将报告模型缺失；查询失败时中止删除并报错。

已经准备好的 Invocation 继续使用其模型请求，连接更新供后续解析使用。独立模型绑定导出时会移除本机引用，导入后重新配置。

## Connection Manager

新建时有两种保存方式，默认名称为模型名：

- 「保存模型」生成 Model Target：只含路由（API、Custom 格式、服务器地址、模型、代理预设、密钥引用）和下文的请求整形，不含预设。
- 「保存模型+预设」生成上游格式的 Connection Profile，只记录模型路由（API、格式、地址、模型、代理预设、密钥、提示词后处理）和设置预设；其余字段（停止字符串、以…开始回复、推理模板、正则预设，以及文本补全的模板类字段）记入 `exclude`。这类 Profile 带有标记 `extensions.tauritavern.kind = 'modelAndPreset'`，`/profile-update` 与编辑都会保留它；编辑时只提供改名，其他 Profile 仍打开上游的字段勾选编辑器。

需要包含全局格式化设置的完整 Profile 仍可用 `/profile-create` 创建，已有 Profile 照常编辑和应用。

下拉框分两组：「模型」组在前，含 Model Target 和没有保存预设的 Connection Profile；「模型+预设」组在后，含保存了预设的 Connection Profile，选择时一并切换预设。AI 响应配置面板顶部的「模型」下拉框是该选择器的镜像，选择仍交给 `#connection_profiles` 处理，见 [紧凑选择行](../FrontendGuide.md#65-紧凑选择行)。

### 连接归属

选中模型时，总由它决定连接；选中 Connection Profile 时，只有它记录了 API 才决定连接。决定连接期间：

- 切换预设不应用预设中的连接字段（`OAI_PRESET_CHANGED_BEFORE` 的 `bindConnection` 被置为 `false`）。
- `#bind_preset_to_connection` 被禁用，`title` 说明原因。
- 保存预设时，连接字段保留预设原有的值：覆盖保存取被覆盖的预设，重命名取原预设，另存为取当前加载的预设；来源预设没有存储的连接字段不写入。

选中项不决定连接或未选中任何项时，由「预设绑定连接」开关决定，与上游一致。开关开启时，导入一个会改变当前路由（来源、模型、Custom 地址与格式、反代）的预设，会先询问「加载并切换连接」还是「只导入预设（去掉连接）」；按 Esc 则按上游行为照常加载。

### 请求整形

模型与「模型+预设」保存时，把当前端点的请求整形记入 `adapterHints`，与物化连接共用同一组键：

- 提示词后处理；
- Custom 端点的 include headers、include body、exclude body；
- Custom Claude Messages 格式的 Claude 提示词缓存，Custom Responses 格式的 WebSocket 模式。

include / exclude 三项在设置中按端点保存在 Additional Parameters 里，读写时使用该项自身格式对应的条目。选中时应用这些值；切换到其他项时，上一项写入的值先恢复为当前加载预设所存的值（预设未存时为空或关闭），再写入新项自己的值。这一恢复不看「预设绑定连接」开关，开关关闭时也会发生，换来的是模型的整形不会残留到下一个连接。选择「无」或删除选中项不改动实时连接，整形也随之保留，与路由保持配套，直到进入下一项。新建或另存为的项从实时设置采集整形，保存后即由它负责这些值。较早保存、缺少某项非开关提示的模型不改动该项。

### 已知限制

- 连接的 `modelId` 是新字段。后端结构拒绝未知字段，较旧的 TauriTavern 版本无法读取含有它的连接文件，不支持降级。

## 实现

- [llm_connection.rs](../../src-tauri/crates/tt-domain/src/models/llm_connection.rs)：连接结构。
- [llm_connection_service.rs](../../src-tauri/crates/tt-application/src/services/llm_connection_service.rs)：解析与请求字段应用。
- [model-target-llm-connection.js](../../src/scripts/tauritavern/agent/model-target-llm-connection.js)：Model Target 同步。
- [connection-manager](../../src/scripts/extensions/connection-manager)：保存、选择、连接归属与请求整形。
