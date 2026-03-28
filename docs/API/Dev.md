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

## 3. `llmApiLogs`

```js
const index = await dev.llmApiLogs.index({ limit: 20 });
const preview = await dev.llmApiLogs.getPreview(index[0].id);
const raw = await dev.llmApiLogs.getRaw(index[0].id);
```

### 方法

| 方法 | 返回值 | 说明 |
| --- | --- | --- |
| `index(options?)` | `Promise<LlmApiLogIndexEntry[]>` | 获取最近几条请求索引 |
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
  level: 'INFO' | 'ERROR';
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
  level: 'INFO' | 'ERROR';
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

## 4. 边界与稳定性

- `tauritavern-backend-log`
- `tauritavern-llm-api-log`
- `devlog_*`

以上事件名和命令名都属于宿主内部实现细节，不是第三方扩展 Public Contract。扩展应只依赖 `api.dev`。
