# Vector 兼容层当前契约

本文记录 TauriTavern 对 SillyTavern 1.18.0 Vector 扩展的原生兼容边界。

## 1. 可观察契约

同源兼容层实现以下 `POST` 路由：

- `/api/vector/list`
- `/api/vector/insert`
- `/api/vector/delete`
- `/api/vector/query`
- `/api/vector/query-multi`
- `/api/vector/purge`
- `/api/vector/purge-all`

前端请求经 `src/tauri/main/routes/vector-routes.js` 汇入一个 typed Tauri command，presentation 只转交 DTO，输入校验、provider 选择、secret 与 iOS policy 均由 `VectorService` 编排。

## 2. 边界与存储

```text
same-origin route
  -> vector_handle
  -> VectorService
     -> VectorRepository             -> tt-adapter-vector / redb
     -> LocalEmbeddingRepository     -> tt-adapter-vector / FastEmbed
     -> RemoteEmbeddingRepository    -> tt-adapter-provider-http
```

索引位于 `default-user/vectors/tauritavern-v1.redb`。模型缓存位于 `_cache/embedding-models/`，purge 只删除索引记录，不重复下载模型。数据库与模型均懒加载；初始化失败只令当前 Vector 请求失败，不影响应用启动。

每个 scope 由 collection、source、model 以及会改变 embedding 空间的 endpoint/account 设置共同确定。插入先完成整个 batch 的 embedding、数量/维度/有限值/非零校验与 L2 归一化，再以单个 redb write transaction 提交 metadata 和 `f32` bytes。相同 hash/index/text 的重试是幂等 upsert；delete 仍按上游 hash 语义删除所有分块。

文件索引以整份文件作为一个 `/insert` 工作单元；`VectorService` 可在内部对 provider 分批请求，但只有全部 embedding 成功后才提交索引，避免可恢复的中途失败留下“已有 hash、实际不完整”的状态。

## 3. 检索基线

当前使用归一化向量的精确 cosine scan，并按分数降序返回。`query-multi` 只计算一次 query embedding，再跨 collection 做全局 threshold 与 top-K。`hashes` 和 `metadata` 始终来自同一组 threshold 后的结果，避免上游实现的数组错位。

聊天生成拦截器在 dense 查询之外并行调用 `window.__TAURITAVERN__.api.chat` 的现有文本检索。两路各取最多 `insert * 4` 个候选（受后端 1000 条上限约束），lexical 路径最多扫描最近 1000 条消息；按 hash 去重后使用 Reciprocal Rank Fusion 合并排名，再截取 `insert` 条。重复 message chunks 不会重复增加同一路排名权重；同时命中 dense 与 lexical 的消息会自然前移。文本检索失败只令本轮退回 dense 结果并留下 console warning，不中止生成。

最终消息按原聊天顺序格式化和注入，而不是按相似度重排叙事顺序；lexical 查询用绝对 `endIndex` 排除 `protect` 范围，最终映射再对 dense 与 lexical 结果共同排除一次。该混合路径只作用于聊天，Data Bank、文件与 World Info 继续使用现有 dense 查询。

这是有意选择的 60/95 基线：聊天与文件集合通常远小于需要 ANN 生命周期的规模。只有真实 profile 显示 collection scan 延迟不可接受时，才在 `VectorRepository` 后替换为 ANN；GraphRAG、知识图谱抽取、reranker 和后台索引任务不属于 SillyTavern Vector 兼容层。

## 4. Embedding source

- `transformers`：Vector 设置可选择以下本地模型，首次使用时下载到 `_cache/embedding-models/` 并缓存；未携带 `model` 的旧调用继续使用 Jina v2，避免升级时静默切换已有索引。
  - `jinaai/jina-embeddings-v2-base-en`：兼容旧默认。
  - `BAAI/bge-m3`：当前只消费 dense 输出。
  - `Qwen/Qwen3-Embedding-0.6B`：query 使用检索 instruction，document 保持原文。
  - `google/embeddinggemma-300m`：使用 FastEmbed 的 Q4 ONNX 版本，query/document 分别使用官方 retrieval prompt。
- `webllm`：沿用前端预计算映射。
- `koboldcpp`：兼容 `/api/backends/kobold/embed`，并把服务端报告的实际 embedding model 带入 Vector scope，避免同一 endpoint 换模型后混合向量空间。
- OpenAI-compatible、Cohere、Nomic、Google AI Studio、Vertex AI、Extras、Ollama、llama.cpp、vLLM：由 provider HTTP adapter 按各自协议发送。

远端 source 在调用前读取 active secret，并遵守 iOS source allowlist 与 endpoint override policy。未知 source、缺失凭据、非法 URL、provider 数量或维度不一致都会显式失败，不创建或部分写入索引。

与上游一致，Vector 兼容层不区分交互式与批量请求，也不额外施加生成期或 embedding 专属超时；调用会等待本地推理或远端请求完成并正常传播错误。

本地模型 id、量化/runtime 版本、最大 token 长度和 prompt 版本共同组成 Vector profile。模型切换不会混合向量空间；repository 同时只保留一个已加载 runtime，切换时释放旧模型以限制常驻内存。
