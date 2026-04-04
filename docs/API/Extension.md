# `window.__TAURITAVERN__.api.extension.store` — API 参考

TauriTavern 为第三方扩展提供的**全局**持久化能力，可选KV JSON 与 Blob 两种数据形态。

数据存储在 `data_root/_tauritavern/extension-store/` 下，可随数据根目录一起备份/迁移/清理。

## 0. 快速上手

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const store = window.__TAURITAVERN__.api.extension.store;
```

### KV JSON 存储

```js
await store.setJson({ namespace: 'my-ext', key: 'settings', value: { enabled: true } });
const settings = await store.getJson({ namespace: 'my-ext', key: 'settings' });
```

### JSON 多表（额外 json / table）

> 不提供 `table` 时默认使用主表 `main`。

```js
await store.setJson({
  namespace: 'my-ext',
  table: 'index',
  key: 'v1',
  value: { lastId: 42 },
});

const tables = await store.listTables({ namespace: 'my-ext' });
```

### Blob 存储

```js
await store.setBlob({
  namespace: 'my-ext',
  key: 'icon.png',
  data: myBlob, // Blob | ArrayBuffer | Uint8Array | base64 string
});

const blob = await store.getBlob({ namespace: 'my-ext', key: 'icon.png' });
```

## 1. KV JSON 方法

所有 KV 方法都支持可选 `table` 字段：

- 省略：使用默认主表 `main`
- 指定：将数据写入对应 table

### 什么时候用 KV JSON？

建议绝大多数情况下使用KV JSON，把它当作“扩展的结构化数据存储”：

- 扩展设置（settings / feature flags）
- 索引、映射表、状态快照（index / map / progress）
- 需要 `updateJson()` 深度合并的场景（只 patch 一小部分字段）

| 方法 | 返回值 | 说明 |
| --- | --- | --- |
| `getJson({ namespace, key, table? })` | `Promise<any>` | 读取 JSON 值 |
| `setJson({ namespace, key, value, table? })` | `Promise<void>` | 写入 JSON 值（覆盖） |
| `updateJson({ namespace, key, value, table? })` | `Promise<void>` | 合并更新：对象会深度合并；非对象直接替换 |
| `renameKey({ namespace, key, newKey, table? })` | `Promise<void>` | 重命名 key |
| `deleteJson({ namespace, key, table? })` | `Promise<void>` | 删除 key |
| `listKeys({ namespace, table? })` | `Promise<string[]>` | 列出 table 下所有 key |
| `listTables({ namespace })` | `Promise<string[]>` | 列出现有 table（KV 与 Blob 的 union） |
| `deleteTable({ namespace, table })` | `Promise<void>` | 删除 table（同时删除该 table 下的 KV 与 Blob） |

兼容：`updateJSON()` 是 `updateJson()` 的别名；`updateKey()` 是 `renameKey()` 的别名。

## 2. Blob 方法

### 什么时候用 Blob？

存大型文件时，需要时再用：

- 图片/音频/压缩包/模型文件等二进制
- 体积较大、不希望浪费 JSON 格式化算力的内容（例如大段纯文本缓存）
- 需要直接喂给 Web API 的场景（返回值是 `Blob`，可 `URL.createObjectURL()`）

| 方法 | 返回值 | 说明 |
| --- | --- | --- |
| `getBlob({ namespace, key, table? })` | `Promise<Blob>` | 读取 blob（文件类型 mimeType 基于 key 文件扩展名猜测） |
| `setBlob({ namespace, key, data, table? })` | `Promise<void>` | 写入 blob |
| `deleteBlob({ namespace, key, table? })` | `Promise<void>` | 删除 blob |
| `listBlobKeys({ namespace, table? })` | `Promise<string[]>` | 列出 table 下所有 blob key |

## 3. 命名规则（namespace / table / key）

- 仅允许字符：`[A-Za-z0-9_.-]`
- 不能为空
- 不能以 `.` 开头
- `.` / `..` 不被允许

## 4. `table` （多表）的语义

`table` 是**逻辑分组**：方便把同一扩展的不同数据域分开放、以及一键清理某个分组（`deleteTable`）。

当前实现中：

- **每个 `(namespace, table, key)` 都是一个独立文件**
- `KV JSON`：`.../kv/<table>/<key>.json`
- `Blob`：`.../blobs/<table>/<key>`（原样文件名，不强制扩展名）

示例（以 `my-ext` 为例）：

- `setJson({ namespace: 'my-ext', key: 'settings', value })`
  - 存储在`data_root/_tauritavern/extension-store/my-ext/kv/main/settings.json`
- `setJson({ namespace: 'my-ext', table: 'index', key: 'v1', value })`
  - 存储在`data_root/_tauritavern/extension-store/my-ext/kv/index/v1.json`
- `setBlob({ namespace: 'my-ext', key: 'icon.png', data })`
  - 存储在`data_root/_tauritavern/extension-store/my-ext/blobs/main/icon.png`

如果你希望“一个 table 就是一个大 JSON 文件”，可以用一个固定 `key` 代表整张表（例如 `key: 'db'`），把内容整体放进 JSON 里即可。
