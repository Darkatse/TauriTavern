# `window.__TAURITAVERN__.api.dev` — API 参考

TauriTavern 为开发者工具、调试面板与扩展作者提供的规范化调试 API。

> 设计目标：让调用方依赖稳定宿主 ABI，而不是直接依赖 Tauri 事件名、Rust 命令名或某个 Settings 面板的内部实现。

## 0. 快速上手

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const dev = window.__TAURITAVERN__.api.dev;
```

## 1. `frontendLogs`

```js
const entries = await dev.frontendLogs.list({ limit: 50 });

const unsubscribe = await dev.frontendLogs.subscribe((entry) => {
  console.log('[frontend]', entry.level, entry.message);
});
```

### 方法

| 方法 | 返回值 | 说明 |
| --- | --- | --- |
| `list(options?)` | `Promise<FrontendLogEntry[]>` | 获取当前已捕获的前端日志尾部 |
| `subscribe(handler)` | `Promise<unsubscribe>` | 订阅新增前端日志 |
| `getConsoleCaptureEnabled()` | `Promise<boolean>` | 读取 console capture 开关 |
| `setConsoleCaptureEnabled(enabled)` | `Promise<void>` | 设置 console capture 开关 |

### `FrontendLogEntry`

```ts
type FrontendLogEntry = {
  id: number;
  timestampMs: number;
  level: 'debug' | 'info' | 'warn' | 'error';
  message: string;
  target?: string;
};
```

### 语义

- 前端日志 capture 开关由宿主统一管理。
- 调用方不应再自行读写相关 `localStorage` key。
- `unsubscribe` 可安全重复调用。
- 页面开始卸载（非 bfcache 的 `pagehide`）后，宿主不再捕获日志，订阅者也不会在页面销毁过程中被调用。
- 为避免 DEBUG/INFO 噪声挤掉关键告警，宿主按级别独立保留：`debug` 400 条、`info` 300 条、`warn`+`error` 共 100 条。保留区比三者之和多出 100 条时修剪一次，所以 `list()` 可能多返回至多 100 条较早的条目。
- 订阅者在每条日志写入时同步调用，开启 console capture 后一次提示词组装可能写入上千条。处理器只做常数时间的工作，界面更新按帧合并。
- `message` 是用于 UI 展示的 **preview**：可能被截断；对象参数会被摘要化。需要完整请求/响应体请使用 `llmApiLogs.getRaw()` 或导出日志。

## 2. `backendLogs`

```js
const recent = await dev.backendLogs.tail({ limit: 100 });

const unsubscribe = await dev.backendLogs.subscribe((entry) => {
  console.log('[backend]', entry.target, entry.message);
});
```

### 方法

| 方法 | 返回值 | 说明 |
| --- | --- | --- |
| `tail(options?)` | `Promise<BackendLogEntry[]>` | 获取当前后端日志尾部 |
| `subscribe(handler)` | `Promise<unsubscribe>` | 订阅新增后端日志 |

### `BackendLogEntry`

```ts
type BackendLogEntry = {
  id: number;
  timestampMs: number;
  level: 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';
  target: string;
  message: string;
};
```

### 语义

- 宿主负责共享后端日志流。
- 多个订阅者并存时，底层流的启停由宿主统一引用计数，不应互相踩踏。
- `message` 可能被截断以保证性能；完整排查请以文件日志/导出 bundle 为准。

## 3. `llmApiLogs`

```js
const index = await dev.llmApiLogs.index({ limit: 20 });
const preview = await dev.llmApiLogs.getPreview(index[0].id);
const raw = await dev.llmApiLogs.getRaw(index[0].id);
```

### 方法

| 方法 | 返回值 | 说明 |
| --- | --- | --- |
| `index(options?)` | `Promise<LlmApiLogIndexEntry[]>` | 获取最近几条请求索引，按时间从旧到新 |
| `getPreview(id)` | `Promise<LlmApiLogPreview>` | 获取适合 UI 展示的预览 |
| `getRaw(id)` | `Promise<LlmApiLogRaw>` | 获取完整原始请求/响应 |
| `subscribeIndex(handler)` | `Promise<unsubscribe>` | 订阅新增索引项 |
| `getKeep()` | `Promise<number>` | 读取保留条数设置 |
| `setKeep(value)` | `Promise<void>` | 设置保留条数 |

### `LlmApiLogIndexEntry`

```ts
type LlmApiLogIndexEntry = {
  id: number;
  timestampMs: number;
  level: 'INFO' | 'WARN' | 'ERROR';
  ok: boolean;
  source: string;
  model: string | null;
  endpoint: string;
  durationMs: number;
  stream: boolean;
};
```

### `LlmApiLogPreview`

```ts
type LlmApiLogPreview = {
  id: number;
  timestampMs: number;
  level: 'INFO' | 'WARN' | 'ERROR';
  ok: boolean;
  source: string;
  model: string | null;
  endpoint: string;
  durationMs: number;
  stream: boolean;
  errorMessage: string | null;
  requestReadable: string;
  responseReadable: string;
  responseRawKind: 'json' | 'sse' | null;
};
```

### `LlmApiLogRaw`

```ts
type LlmApiLogRaw = {
  id: number;
  requestRaw: string;
  responseRaw: string;
  responseRawKind: 'json' | 'sse' | null;
};
```

### 语义

- `getPreview()` / `getRaw()` 每次从磁盘读取，不常驻内存。`index()` 与 `subscribeIndex()` 给出的条目可以立即读取，记录规则见[请求查看器](../CurrentState/RequestInspector.md#5-记录)。
- `requestRaw` 是实际发出的请求体，内联媒体换成了占位标记。
- `responseRawKind` 是响应文件的种类：转发事件流的请求是 `sse`；一次取回完整响应的请求成功时是 `json`，失败时没有响应体。`stream` 只表示请求是否以流式发出。
- `requestReadable` 与 `responseReadable` 由结构化视图渲染，不截断，内容很大时会拖慢界面。
- 可读文本的标记与服务商的线路格式无关：消息以 `[<role>]` 开头，可见的 reasoning 是 `[reasoning]` 块，签名等续接状态只显示 `[native_state kind=…]`。
- 工具调用的参数是对象时，其中含换行的字符串值按 `[key]\nvalue` 块展开。

## 4. `requestInspector`（Project Contract）

供第一方请求查看器使用的结构化视图与预览接口。它是 Project Contract，形状随第一方界面演进。

```js
const entry = await dev.requestInspector.getLogView(index[0].id);
const next = await dev.requestInspector.preview();
```

### 方法

| 方法 | 返回值 | 说明 |
| --- | --- | --- |
| `getLogView(id)` | `Promise<LlmApiLogView>` | 读取单条已记录请求的结构化请求/响应视图（`ExchangeView`） |
| `preview()` | `Promise<RequestPreview>` | 给出下一次发送的请求与它的组成，不发送，也不写聊天记录 |

### 语义

- `getLogView()` 返回索引条目的字段、`responseRawKind` 与 `view: { request, response? }`，类型见 `src/types.d.ts` 的 `TauriTavernLlmApiLogView`。
- `path` 是 RFC 6901 JSON Pointer。请求里的指向 `requestRaw`，非流式响应里的指向 `responseRaw`；流式响应由事件合并而来，没有 `path`。
- 工具调用的参数是 JSON 字符串时解析为 JSON 值；解析失败时 `arguments` 为 `null`，原文在 `argumentsText`。
- `view.request.http` 是请求描述，凭据的值是 `[redacted]`，没有描述的记录不带这个字段，见[请求查看器](../CurrentState/RequestInspector.md#4-线路请求)。
- 失败信息只在 `view.response.error`。它与结束原因、用量的取值见[请求查看器](../CurrentState/RequestInspector.md#6-视图)。
- `preview()` 只支持聊天补全下打开的角色聊天。条件不满足，或组装、准备失败时，它抛出错误。
- `preview()` 返回的 `source`、`model`、`endpoint`、`requestRaw` 与 `view` 取值与日志相同，`requestReadable` 与 `llmApiLogs.getPreview()` 的同名字段出自同一个渲染。其余字段见下表，类型见 `src/types.d.ts` 的 `TauriTavernRequestPreview`。它与随后的发送可以不同之处见[请求查看器](../CurrentState/RequestInspector.md#3-准备与预览)。

| 字段 | 含义 |
| --- | --- |
| `promptSources`、`toolSources` | 每条消息与每个工具的出处，见[请求查看器](../CurrentState/RequestInspector.md#2-组装) |
| `promptNames` | 出处里各个 Prompt Manager 提示词的名称，按标识查 |
| `chatHistory` | 纳入的聊天记录条数与候选条数 |
| `mcpDiagnostics` | 这次用不上的 MCP 工具与原因 |
| `assemblyError` | 组装带着错误继续时的原因，没有时为 `null` |

## 5. `exportBundle()`

```js
const delivered = await dev.exportBundle();
```

### 方法

| 方法 | 返回值 | 说明 |
| --- | --- | --- |
| `exportBundle()` | `Promise<boolean>` | 生成并交付 debug bundle（zip）；取消返回 `false`，失败抛出错误 |

### 语义

- 由用户在系统保存面板中选择位置（iOS 为分享面板），见[文件传输](../CurrentState/FileTransfer.md)。
- 导出内容包含后端文件日志、前端日志 snapshot、LLM API Logs（含 raw）与设置/版本信息。
- 设置副本不含密钥文件；键名表明是凭据的值（password、API key、token、auth、headers 等）替换为 `[redacted]`，URL 中的用户名与密码被去掉。
- LLM API Logs 的 meta 经过同样的替换，记录的请求头整体换掉；请求体与响应体原样导出。
- 导出文件可能包含 prompt/响应体等敏感信息；分享前请自行检查。

## 6. 边界与稳定性

- `tauritavern-backend-log`
- `tauritavern-llm-api-log`
- `devlog_*`
- `preview_chat_completion_request`

以上事件名和命令名都属于宿主内部实现细节，不是第三方扩展 Public Contract。扩展应只依赖 `api.dev` 中的 Public Contract。
