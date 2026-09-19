# Database API

`window.__TAURITAVERN__.api.db` 提供基于 TriviumDB 0.8.8 的本地向量、JSON、图关系、文本索引和 TQL。

```js
await window.__TAURITAVERN__.ready;
const db = await window.__TAURITAVERN__.api.db.open('my-extension', {
    dim: 2,
    syncMode: 'full',
});
const id = await db.insert([1, 0], { text: 'Alice knows the password' });
await db.indexText(id, 'Alice knows the password');
await db.indexKeyword(id, 'Alice');
await db.buildTextIndex();
const hits = await db.search([1, 0], { queryText: 'Alice', topK: 5 });
```

完整类型见 [db-types.d.ts](../../src/tauri/main/api/db-types.d.ts)。数据属于当前 data root，命名空间之间物理隔离；同一 WebView 内的扩展仍共享宿主信任边界。

## 打开与生命周期

`open(namespace, options?) -> Promise<DatabaseHandle>` 等待数据库真正可用，失败 reject。命名空间为 1–128 位小写 ASCII 字母、数字、`-`、`_`。每个命名空间使用一个数据库实例。

| 选项 | 初次打开默认值 | 含义 |
| --- | --- | --- |
| dim | 新库 1536 | 已有数据库省略时读取实际维度；显式提供时必须一致 |
| storageMode | mmap | mmap 或 rom；由原生存储实现解释 |
| syncMode | normal | normal、full、off；沿用原生 WAL 同步语义 |
| loadTextIndex | true | 文本索引预加载偏好，见下文 |
| autoBuildQuiver | true | 原生自动索引构建 |
| memoryLimitMb | 0 | 原生内存预算，0 表示不设置预算；不是整个进程 RSS 上限 |

同名数据库已打开时，未提供的选项沿用该实例的配置；显式提供的不同维度、存储/同步模式、内存预算或自动构建选项会报冲突，调用方可关闭后重新打开。预加载只发生在实际打开时，不会重新加载已打开的实例。

handle 的 `namespace`、`dim`、`options` 是本次打开的有效配置快照。0.8.8 会在 flush 时写回内存文本索引，因此已有文本 sidecar 时，宿主始终加载它以防覆盖原有内容；即使传了 `loadTextIndex: false`，返回的有效配置也是 true。

`db.close()` 关闭共享命名空间，影响该命名空间的所有 handle。关闭失败保留实例，允许重试。关闭后的操作报未打开；重新 `open` 后可以继续使用该命名空间。`api.db.listNamespaces()` 仅列当前打开的命名空间。

应用进入后台时尝试 flush；正常退出前等待 flush。退出流程因其他保存失败而保留应用时，数据库仍可继续使用。移动系统强制结束进程不保证执行这些回调。

## 节点、图与文本

NodeId 在所有接口中均为 JS `number`，按非负安全整数使用（不超过 `Number.MAX_SAFE_INTEGER`，即 9,007,199,254,740,991）。返回的 ID 可直接用于 CRUD、图操作和 TQL 参数。payload 是普通 JSON，宿主不重写其字段。

| 方法 | 结果 / 语义 |
| --- | --- |
| insert(vector, payload?) | 新 NodeId |
| batchInsert(vectors, payloads) | NodeId 数组；长度必须一致，整批原子提交 |
| upsert(id, vector, payload?) | 按指定 ID 插入或覆盖 |
| get(id) | `{ id, vector, payload, edges }` 或 null |
| updatePayload(id, payload) | 替换 payload |
| patchPayload(id, patch) | 原生 `$set`、`$inc`、`$unset` 等 patch 语义 |
| updateVector(id, vector) | 更新向量 |
| delete(id) | 删除节点 |
| link(src, dst, label='related', weight=1) | 创建/更新关系 |
| unlink(src, dst) | 移除该节点对之间的边 |
| shortestPath(source, target, options?) | NodeId 数组或 null；options 为 maxDepth、label |
| subgraph(id, options?) | nodes、edges；options 为 maxDepth、labels、direction |
| indexText(id, text) | 添加原生文本索引内容 |
| indexKeyword(id, keyword) | 添加原生关键词索引内容 |
| buildTextIndex() | 编译文本索引并执行一次 flush |

`direction` 为 outgoing、incoming、both。边字段为 targetId、label、weight、metadata；子图边额外包含 sourceId。

**文本索引的持久化边界：** 0.8.8 的手工文本/关键词索引写入不进入 WAL，且原生引擎把文本 sidecar 保存失败视为可恢复索引错误。调用方应把可重建的源文本保存在 payload 或自身持久数据中；批量建立索引后调用 buildTextIndex。正常落盘和重开会保留索引，但 syncMode=full 不会把手工索引操作变成 WAL 事务。节点、向量、payload、关系继续使用原生事务/WAL。宿主不维护第二份索引日志。

## 检索

- `search(vector=null, options={}) -> Hit[]`：提供 queryText 时默认启用文本召回。保留 filter 作为 payloadFilter 的便利参数；若两者都提供，以 payloadFilter 为准。
- `searchBatch(vectors, options={}) -> Hit[][]`：一批向量交给原生批量检索；parallelism 为原生并行设置，0 使用原生默认。
- `searchAdvanced(vector=null, config={}) -> { hits, context }`：默认开启高级管线，返回 timingsMs、stageCounts、observations。具体阶段仍受对应开关控制。

Hit 为 `{ id, score, payload }`。topK 默认 5；其他搜索字段基于原生默认值，仅覆盖调用者明确提供的值。基础搜索默认关闭高级总开关，高级搜索默认开启，均可通过 enableAdvancedPipeline 显式指定。queryText 单独传给引擎。

SearchOptions 保留 recallK、rerankK、expandDepth、expandLabels、maxEdgesPerNode、minEdgeWeight、edgeDirection、minScore、teleportAlpha、enableSparseResidual、fistaLambda、fistaThreshold、enableDpp、dppQualityWeight、enableRefractoryFatigue、enableInverseInhibition、lateralInhibitionThreshold、forceBruteForce、enableTextHybridSearch、textBoost、bm25K1、bm25B、payloadFilter、diffusionBias。

## TQL

```js
const result = await db.query('SEARCH VECTOR $vec TOP 5 RETURN *', { vec: [1, 0] });
const written = await db.query('CREATE ($payload)', { payload: { text: 'new memory' } });
const byId = await db.query('MATCH (n) WHERE n.id == $id RETURN n', { id });
```

`query(text, params={})` 保留读写语句、整向量、标量和 JSON 字面量参数。宿主使用原生 lexer 的完整 token 边界展开参数，跳过字符串和注释；原生 parser 判定读写。参数里的 `$name` 不会二次展开，`$id` 不会影响 `$id2`，`$gte:` 等文档过滤键保持原意。缺少实际引用的参数时报错；多余参数不影响执行。

参数是值，不能用于替换关键字、字段名或原生过滤操作符。可使用的位置和数据结构仍由 TQL 语法决定；例如 0.8.8 的 CREATE 属性值不支持任意嵌套对象，复杂 JSON 节点写入可直接使用 insert/upsert。

参数直接使用普通 JSON 值；数值参数保持数值，字符串参数保持字符串。整数按 JS 安全整数范围使用，无需额外类型包装。

结果为：

```ts
{ type: 'mutation', affected: number, createdIds: number[] }
// 或
{ type: 'query', rows: Array<Record<string, QueryValue>> }
```

每个 QueryValue 有 type 与相应 value：node、edge、integer、float、string、bool、path、list；null 只有 type。integer 和 float 的 value 均为 JS number，list 为普通 JSON 数组，节点 payload 也是普通 JSON。数值沿用 JS number 的精度范围。

## 维护、错误与数据迁移

- `flush()`：显式落盘。
- `compact()`：原生压缩整理。
- `buildQuiverIndex()`：显式构建 QuIVer。
- `stats()`：namespace、dim、nodeCount、estimatedMemoryBytes、graph。

输入、未打开、冲突与实际 IO/恢复错误通过 Promise rejection 返回。调用方可以修正配置或数据后重试；宿主不自动重试写入，不为查询设置缓存、去重或延迟写回。停止等待不等于取消已经进入原生执行的操作。

数据位于 `_tauritavern/databases/db-<namespace>/`。归档导入按 namespace 整体替换，保留备份未包含的 namespace。

归档和数据库同步期间暂停数据库操作。导入或同步接收会关闭实例，任务结束后（包括失败或取消）调用方需重新 `open(namespace)`。

同步范围与覆盖策略见 [Sync](../CurrentState/Sync.md#triviumdb数据库)。
