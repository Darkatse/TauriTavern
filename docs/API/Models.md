# Models API

`api.models` 列出用户在 Connection Manager 中保存的模型（Model Target），也就是 Agent Profile 和 App assistant 可选的那一份列表。它只读，不含密钥；保存、改名、删除仍在 Connection Manager 中完成。没有保存预设的 Connection Profile 虽然也显示在下拉框的「模型」组，但不在本列表中。

状态：Project Contract（实验性），签名可能调整。

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const models = window.__TAURITAVERN__.api.models;
```

| 方法 | 返回值 |
| --- | --- |
| `list()` | `Promise<{ models }>`，按名称排序 |
| `get(ref)` | `Promise`，单个模型或 `null`；`ref` 可以是 `{ kind, id }`、`modelTarget:<id>` 或原始 id |
| `subscribe(listener)` | 同步返回取消订阅函数；模型新建、更新、删除或 Connection Manager 选择变化时调用 `listener({ models })`，订阅时不立即调用 |

`list()` 与 `get()` 当前读取前端设置，写成异步是为了日后改由后端提供时调用方不必改动。

## 模型内容

```ts
type ModelSummary = {
  ref: { kind: 'tauritavern.modelTarget'; id: string }; // 本机引用
  requestId: string;            // modelTarget:<id>
  connectionRef: string | null; // Agent Profile 的 connectionRef；文本补全模型为 null
  name: string;
  mode: 'cc' | 'tc';
  source: string;               // 规范化后的渠道名，见下
  apiFormat: string | null;     // Custom / OpenCode 的格式，其他渠道为 null
  model: string;
  selected: boolean;            // 是否为 Connection Manager 当前选中项
};
```

`source` 是该模型的 Agent 连接实际使用的 chat-completion source，按构建连接时的规则规范化（如 `google` → `makersuite`，`custom_claude_messages` 等别名 → `custom`）；文本补全模型为其 API 名称。`apiFormat` 同样规范化：Custom 未记录格式时为 `openai_compat`（或由 API 别名推出），OpenCode 未记录时为 `openai_compat`。

引用只在本机有效。需要导出的配置应在导入后让用户重新选择，做法与 Agent Profile 的 `requiresConfiguration` 相同。

## 用选中的模型发请求

不切换用户当前连接时，把 `requestId` 交给 SillyTavern 的 `ConnectionManagerRequestService`：

```js
const { ConnectionManagerRequestService } = SillyTavern.getContext();
const [model] = (await models.list()).models;
const result = await ConnectionManagerRequestService.sendRequest(model.requestId, 'Hello', 256);
```

模型视为没有预设的只读 profile 视图：路由、密钥引用、提示词后处理和 Custom 请求整形都来自模型本身。

- `getProfile` 与 `findProfile` 接受 `modelTarget:<id>`。
- `getSupportedProfiles({ includeModelTargets: true })` 同时返回模型。
- `handleDropdown(selector, id, onChange, onCreate, onUpdate, onDelete, { includeModelTargets: true })` 在扩展自己的下拉框里加入「模型」组。模型的新建、更新、删除同样调用 `onCreate` / `onUpdate` / `onDelete`，参数为模型的只读 profile 视图；重建分组时保留当前选择，选中的模型被更新或删除时重新触发 `onChange`（删除后不带 profile）。默认不加入，因为按上游写法的扩展可能直接在 Connection Profile 列表中查找保存的 id。

`/profile <名称>` 与 `/profile-get <名称>` 也能按名称找到模型：先精确匹配 Connection Profile，再精确匹配模型，然后模糊匹配，同级时 Connection Profile 优先，上游用法的结果不变；不带参数时返回当前选中项（`/profile-get` 对模型返回只读 profile 视图）。`/profile-genstream profile=` 先按 id（含 `modelTarget:<id>`）查找，再沿用上游的模糊搜索，同分时 Connection Profile 优先。

## 连接由谁决定

选中模型，或选中记录了 API 的 Connection Profile（含「模型+预设」）时，由选中项决定连接：切换预设不改变连接，保存预设时保留预设原有的连接字段。选中项不记录 API 或未选中任何项时，由「预设绑定连接」开关决定，与上游一致。实时设置偏离选中项时，行内出现「重新应用」（API 面板还有「覆盖」），选项文字不变。选中项不决定连接且开启绑定时，导入一个会切换当前连接的预设会先询问是加载并切换连接，还是只导入预设。详见 [LLM Connection API](LlmConnections.md#connection-manager)。

Agent Profile 与 App assistant 绑定模型后跟随模型：修改模型的模型名后无需重新选择；文本补全模型在选择器中显示为不可用。

## 实现

- [models.js](../../src/tauri/main/api/models.js)：本 API。
- [shared.js](../../src/scripts/extensions/shared.js)：`ConnectionManagerRequestService` 对模型的支持。
- [connection-manager](../../src/scripts/extensions/connection-manager)：保存、选择、偏离提示与连接归属。
