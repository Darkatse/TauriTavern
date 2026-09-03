// @ts-check

/**
 * TriviumDB 0.8.5 扩展智能数据库 API
 *
 * 为第三方扩展与宿主提供原生的稠密向量检索、自研 QuIVer 索引、知识图谱拓扑、Rayon 并行批量检索、
 * AC 自动机专有名词索引与 TQL 混合查询能力。
 *
 * 数据按 namespace 物理隔离，每个命名空间对应独立的 .tdb 文件。
 *
 * 使用示例：
 * ```js
 * const db = window.__TAURITAVERN__.api.db.open('memory-plugin', {
 *     dim: 1536,
 *     storageMode: 'mmap',     // 'mmap' 零拷贝极速热启动
 *     syncMode: 'normal',       // 'normal' | 'off'（大批量写入时可设为 off 冲刺吞吐）
 *     autoBuildQuiver: true,    // 自动构建 QuIVer SOTA 向量图索引
 * });
 *
 * // 1. 插入节点
 * const id = await db.insert(vector, { text: '记忆正文', character_id: 'alice' });
 *
 * // 2. AC 自动机精准实体锚点
 * await db.indexKeyword(id, '爱丽丝');
 *
 * // 3. 带 Payload 预过滤的高级认知检索
 * const res = await db.searchAdvanced(queryVector, {
 *     topK: 5,
 *     payloadFilter: { character_id: { $eq: 'alice' } },
 *     enableRefractoryFatigue: true, // 开启不应期抑制，避免死循环记忆
 * });
 * console.log('命中项:', res.hits);
 * console.log('QuIVer 与耗时观测:', res.context);
 *
 * // 4. 多 Query 并发批量检索 (Rayon 并行加速)
 * const batchHits = await db.searchBatch([vec1, vec2, vec3], { topK: 3 });
 *
 * // 5. 关联记忆子图提取 (支持前端 Graph 可视化)
 * const subgraph = await db.subgraph(id, { maxDepth: 2, direction: 'both' });
 *
 * // 6. 直接执行 TQL 混合查询
 * const results = await db.query(`
 *     SEARCH VECTOR $vec TOP 10 AS seed
 *     WITH seed
 *     EXPAND seed [:cites*1..2] AS related
 *     RETURN related
 * `, { vec: queryVector });
 * ```
 */

/**
 * 校验非空命名空间字符串
 * @param {unknown} value
 * @param {string} label
 */
function requireValidNamespace(value, label = 'namespace') {
    const resolved = String(value || '').trim();
    if (!resolved) {
        throw new Error(`${label} 不能为空`);
    }
    return resolved;
}

/**
 * 创建针对单个命名空间的操作句柄
 * @param {{
 *     safeInvoke: (command: any, args?: any) => Promise<any>;
 *     namespace: string;
 *     dim: number;
 *     options?: {
 *         storageMode?: 'mmap' | 'rom';
 *         syncMode?: 'normal' | 'full' | 'off';
 *         loadTextIndex?: boolean;
 *         autoBuildQuiver?: boolean;
 *         memoryLimitMb?: number;
 *     };
 * }} deps
 */
function createDbHandle({ safeInvoke, namespace, dim, options }) {
    // 异步预热打开（若已打开则复用，支持指定引擎存储模式与同步等级）
    safeInvoke('trivium_open', {
        namespace,
        options: {
            dim,
            storageMode: options?.storageMode,
            syncMode: options?.syncMode,
            loadTextIndex: options?.loadTextIndex,
            autoBuildQuiver: options?.autoBuildQuiver,
            memoryLimitMb: options?.memoryLimitMb,
        },
    }).catch((err) => {
        console.warn(`[TriviumDB] 预热打开命名空间 ${namespace} 警告:`, err);
    });

    return {
        /** 当前命名空间 */
        namespace,
        /** 当前向量维度 */
        dim,

        // ── 节点 CRUD 与 Upsert ──

        /**
         * 插入单个节点（自动生成递增唯一 ID）
         * @param {number[]} vector 向量数组（长度必须与 dim 一致）
         * @param {any} [payload] 挂载的 JSON 业务元数据
         * @returns {Promise<number>} 新节点 ID
         */
        insert: (vector, payload = null) =>
            safeInvoke('trivium_insert', { namespace, dim, vector, payload }),

        /**
         * 批量插入节点
         * @param {number[][]} vectors 向量列表
         * @param {any[]} payloads 元数据列表
         * @returns {Promise<number[]>} 生成的节点 ID 列表
         */
        batchInsert: (vectors, payloads) =>
            safeInvoke('trivium_batch_insert', { namespace, dim, vectors, payloads }),

        /**
         * 使用指定 ID 原子插入或覆盖更新节点（Upsert）
         * @param {number} id 节点 ID
         * @param {number[]} vector 向量数组
         * @param {any} [payload] 元数据
         */
        upsert: (id, vector, payload = null) =>
            safeInvoke('trivium_upsert_with_id', { namespace, dim, id, vector, payload }),

        /**
         * 读取指定节点及其全部关联边
         * @param {number} id 节点 ID
         * @returns {Promise<{ id: number, vector: number[], payload: any, edges: Array<{ target_id: number, label: string, weight: number, metadata: any }> } | null>}
         */
        get: (id) =>
            safeInvoke('trivium_get', { namespace, dim, id }),

        /**
         * 全量更新节点的元数据 Payload
         * @param {number} id 节点 ID
         * @param {any} payload 新的 JSON 元数据
         */
        updatePayload: (id, payload) =>
            safeInvoke('trivium_update_payload', { namespace, dim, id, payload }),

        /**
         * 局部更新节点的元数据 Payload（支持 $set, $inc 等操作）
         * @param {number} id 节点 ID
         * @param {any} patch 补丁对象
         */
        patchPayload: (id, patch) =>
            safeInvoke('trivium_patch_payload', { namespace, dim, id, patch }),

        /**
         * 更新节点的稠密向量
         * @param {number} id 节点 ID
         * @param {number[]} vector 新向量数组
         */
        updateVector: (id, vector) =>
            safeInvoke('trivium_update_vector', { namespace, dim, id, vector }),

        /**
         * 删除指定节点（自动清理拓扑边关系与向量索引）
         * @param {number} id 节点 ID
         */
        delete: (id) =>
            safeInvoke('trivium_delete', { namespace, dim, id }),

        // ── 知识图谱、子图提取与路径计算 ──

        /**
         * 在两个节点间建立有向带权边
         * @param {number} src 源节点 ID
         * @param {number} dst 目标节点 ID
         * @param {string} [label='related'] 关系标签
         * @param {number} [weight=1.0] 边权重
         */
        link: (src, dst, label = 'related', weight = 1.0) =>
            safeInvoke('trivium_link', { namespace, dim, src, dst, label, weight }),

        /**
         * 移除两个节点之间的图谱边
         * @param {number} src 源节点 ID
         * @param {number} dst 目标节点 ID
         */
        unlink: (src, dst) =>
            safeInvoke('trivium_unlink', { namespace, dim, src, dst }),

        /**
         * 计算两点间最短路径
         * @param {number} source 起点 ID
         * @param {number} target 终点 ID
         * @param {{ maxDepth?: number, label?: string }} [options]
         * @returns {Promise<number[] | null>} 路径节点 ID 列表
         */
        shortestPath: (source, target, options = {}) =>
            safeInvoke('trivium_shortest_path', {
                namespace,
                dim,
                source,
                target,
                maxDepth: options.maxDepth,
                label: options.label,
            }),

        /**
         * 提取以指定节点为中心的关联子图网络（包含节点集与边集，非常适合可视化网络）
         * @param {number} id 中心节点 ID
         * @param {{ maxDepth?: number, labels?: string[], direction?: 'outgoing' | 'incoming' | 'both' }} [options]
         * @returns {Promise<{ nodes: Array<{ id: number, payload: any }>, edges: Array<{ source_id: number, target_id: number, label: string, weight: number, metadata: any }> }>}
         */
        subgraph: (id, options = {}) =>
            safeInvoke('trivium_subgraph', {
                namespace,
                dim,
                id,
                maxDepth: options.maxDepth,
                labels: options.labels,
                direction: options.direction,
            }),

        // ── 全文倒排与 AC 自动机关键词索引 ──

        /**
         * 为节点追加 BM25 全文倒排索引文本
         * @param {number} id 节点 ID
         * @param {string} text 文本内容
         */
        indexText: (id, text) =>
            safeInvoke('trivium_index_text', { namespace, dim, id, text }),

        /**
         * 为节点登记专有名词/实体精准关键词（利用 AC 自动机多模式极速命中）
         * @param {number} id 节点 ID
         * @param {string} keyword 实体关键词（如人名、地名、道具名）
         */
        indexKeyword: (id, keyword) =>
            safeInvoke('trivium_index_keyword', { namespace, dim, id, keyword }),

        /**
         * 编译生成全文倒排索引
         */
        buildTextIndex: () =>
            safeInvoke('trivium_build_text_index', { namespace, dim }),

        // ── 向量与高级认知混合检索 ──

        /**
         * 混合检索（支持向量召回、图扩散、Payload 预过滤与偏置）
         * @param {number[] | null} [vector] 查询向量
         * @param {{
         *     queryText?: string,
         *     topK?: number,
         *     expandDepth?: number,
         *     minScore?: number,
         *     filter?: any,
         *     diffusionBias?: number[]
         * }} [options]
         * @returns {Promise<Array<{ id: number, score: number, payload: any }>>}
         */
        search: (vector = null, options = {}) =>
            safeInvoke('trivium_search', {
                namespace,
                dim,
                vector,
                queryText: options.queryText,
                topK: options.topK,
                expandDepth: options.expandDepth,
                minScore: options.minScore,
                filter: options.filter,
                diffusionBias: options.diffusionBias,
            }),

        /**
         * Rayon 多线程并行批量向量检索（极高并发加速，支持同时检索多个 Query 向量）
         * @param {number[][]} vectors 查询向量列表
         * @param {{
         *     topK?: number,
         *     expandDepth?: number,
         *     minScore?: number,
         *     parallelism?: number
         * }} [options]
         * @returns {Promise<Array<Array<{ id: number, score: number, payload: any }>>>}
         */
        searchBatch: (vectors, options = {}) =>
            safeInvoke('trivium_search_batch', {
                namespace,
                dim,
                vectors,
                topK: options.topK,
                expandDepth: options.expandDepth,
                minScore: options.minScore,
                parallelism: options.parallelism,
            }),

        /**
         * 完整高级认知管线检索（FISTA + SA-PPR 扩散 + DPP 采样 + 详细阶段耗时与 QuIVer 观测指标）
         * @param {number[] | null} [vector] 查询向量
         * @param {object} [config] 认知管线配置矩阵（包含 payloadFilter, diffusionBias, enableRefractoryFatigue 等）
         * @returns {Promise<{
         *     hits: Array<{ id: number, score: number, payload: any }>,
         *     context: {
         *         timings_ms: Record<string, number>,
         *         stage_counts: Record<string, number>,
         *         observations: Record<string, number>
         *     }
         * }>}
         */
        searchAdvanced: (vector = null, config = {}) =>
            safeInvoke('trivium_search_advanced', {
                namespace,
                dim,
                vector,
                queryText: config.queryText,
                config,
            }),

        // ── TQL 统一查询引擎 ──

        /**
         * 执行原生 TQL（Trivium Query Language）语句
         *
         * 支持 FIND、MATCH、SEARCH VECTOR、EXPAND、pagerank 图算法、路径与 Mutation 写入
         * @param {string} query TQL 查询语句
         * @param {Record<string, any>} [params] 参数字典（用于 $param 占位符安全替换）
         * @returns {Promise<{ type: 'query' | 'mutation', rows?: any[], affected?: number, createdIds?: number[] }>}
         */
        query: (query, params = {}) => {
            let processedQuery = query;
            if (params && typeof params === 'object') {
                for (const [k, v] of Object.entries(params)) {
                    const placeholder = `$${k}`;
                    const jsonVal = JSON.stringify(v);
                    processedQuery = processedQuery.split(placeholder).join(jsonVal);
                }
            }
            return safeInvoke('trivium_query_tql', {
                namespace,
                dim,
                query: processedQuery,
            });
        },

        // ── 高性能维护与管理 ──

        /**
         * 主动构建/更新 QuIVer 向量索引（BQ+Vamana 拓扑）
         */
        buildQuiverIndex: () =>
            safeInvoke('trivium_build_quiver_index', { namespace, dim }),

        /**
         * 压缩整理数据库（合并重写 WAL 日志、回收墓碑内存碎片）
         */
        compact: () =>
            safeInvoke('trivium_compact', { namespace, dim }),

        /**
         * 手动将脏数据持久化落盘
         */
        flush: () =>
            safeInvoke('trivium_flush', { namespace }),

        /**
         * 关闭并释放数据库句柄
         */
        close: () =>
            safeInvoke('trivium_close', { namespace }),

        /**
         * 获取数据库节点规模、图谱与内存统计指标
         * @returns {Promise<{ namespace: string, dim: number, nodeCount: number, estimatedMemoryBytes: number, graph: { nodeCount: number, edgeCount: number } }>}
         */
        stats: () =>
            safeInvoke('trivium_stats', { namespace, dim }),
    };
}

/**
 * 创建全局 db API 根对象
 * @param {{ safeInvoke: (command: any, args?: any) => Promise<any> }} deps
 */
function createDbApi({ safeInvoke }) {
    return {
        /**
         * 打开或预热一个 TriviumDB 命名空间
         *
         * @param {string} namespace 命名空间名称（仅允许字母、数字、短划线与下划线）
         * @param {{
         *     dim?: number,
         *     storageMode?: 'mmap' | 'rom',
         *     syncMode?: 'normal' | 'full' | 'off',
         *     loadTextIndex?: boolean,
         *     autoBuildQuiver?: boolean,
         *     memoryLimitMb?: number
         * }} [options] 选项参数（向量维度 dim 默认为 1536，存储模式默认为 mmap 零拷贝）
         * @returns 数据库操作句柄
         */
        open(namespace, options = {}) {
            const validNamespace = requireValidNamespace(namespace, 'namespace');
            const dim = options?.dim || 1536;
            return createDbHandle({ safeInvoke, namespace: validNamespace, dim, options });
        },

        /**
         * 列出所有当前已打开的命名空间列表
         * @returns {Promise<string[]>}
         */
        listNamespaces() {
            return safeInvoke('trivium_list_namespaces');
        },
    };
}

/**
 * 向宿主环境安装 window.__TAURITAVERN__.api.db
 * @param {any} context Tauri 上下文
 */
export function installDbApi(context) {
    const hostWindow = /** @type {any} */ (window);
    const hostAbi = hostWindow.__TAURITAVERN__;
    if (!hostAbi || typeof hostAbi !== 'object') {
        throw new Error('宿主 ABI __TAURITAVERN__ 缺失');
    }

    const safeInvoke = context?.safeInvoke;
    if (typeof safeInvoke !== 'function') {
        throw new Error('Tauri 主上下文 safeInvoke 函数缺失');
    }

    if (!hostAbi.api || typeof hostAbi.api !== 'object') {
        hostAbi.api = {};
    }

    hostAbi.api.db = createDbApi({ safeInvoke });
}
