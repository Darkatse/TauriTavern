// @ts-check

/**
 * TriviumDB 扩展 API
 *
 * 为第三方扩展提供向量检索、图谱操作和数据持久化能力。
 * 所有数据按 namespace 隔离，每个扩展拥有独立的 .tdb 文件。
 *
 * 使用方式：
 * ```js
 * const db = window.__TAURITAVERN__.api.db.open('my-extension', { dim: 1536 });
 * const id = await db.insert(vector, { text: '记忆内容' });
 * const hits = await db.search(queryVector, { topK: 5, expandDepth: 2 });
 * ```
 */

/**
 * @param {{ safeInvoke: (command: any, args?: any) => Promise<any>; namespace: string; dim: number }} deps
 */
function createDbHandle({ safeInvoke, namespace, dim }) {
    return {
        /** 当前命名空间 */
        namespace,

        // ── 节点 CRUD ──

        /**
         * 插入节点（自动分配 ID）
         * @param {number[]} vector 向量数组（长度必须与 dim 一致）
         * @param {any} payload 挂载的 JSON 元数据
         * @returns {Promise<number>} 新节点 ID
         */
        insert: (vector, payload) =>
            safeInvoke('trivium_insert', { namespace, dim, vector, payload }),

        /**
         * 批量插入节点
         * @param {number[][]} vectors 向量数组列表
         * @param {any[]} payloads payload 列表
         * @returns {Promise<number[]>} 新节点 ID 列表
         */
        batchInsert: (vectors, payloads) =>
            safeInvoke('trivium_batch_insert', { namespace, dim, vectors, payloads }),

        /**
         * 获取节点信息
         * @param {number} id 节点 ID
         * @returns {Promise<{id: number, vector: number[], payload: any, num_edges: number} | null>}
         */
        get: (id) =>
            safeInvoke('trivium_get', { namespace, dim, id }),

        /**
         * 更新节点的 payload（不影响向量和图谱关系）
         * @param {number} id 节点 ID
         * @param {any} payload 新的 payload
         */
        updatePayload: (id, payload) =>
            safeInvoke('trivium_update_payload', { namespace, dim, id, payload }),

        /**
         * 更新节点的向量（不影响 payload 和图谱关系）
         * @param {number} id 节点 ID
         * @param {number[]} vector 新向量
         */
        updateVector: (id, vector) =>
            safeInvoke('trivium_update_vector', { namespace, dim, id, vector }),

        /**
         * 删除节点（同时清除向量、payload 和所有关联边）
         * @param {number} id 节点 ID
         */
        delete: (id) =>
            safeInvoke('trivium_delete', { namespace, dim, id }),

        // ── 图谱操作 ──

        /**
         * 在两个节点之间建立有向带权边
         * @param {number} src 源节点 ID
         * @param {number} dst 目标节点 ID
         * @param {string} [label='related'] 边的标签
         * @param {number} [weight=1.0] 边的权重
         */
        link: (src, dst, label, weight) =>
            safeInvoke('trivium_link', { namespace, dim, src, dst, label, weight }),

        /**
         * 移除两个节点之间的边
         * @param {number} src 源节点 ID
         * @param {number} dst 目标节点 ID
         */
        unlink: (src, dst) =>
            safeInvoke('trivium_unlink', { namespace, dim, src, dst }),

        // ── 检索 ──

        /**
         * 向量检索 + 图谱扩散
         * @param {number[]} vector 查询向量
         * @param {{ topK?: number, expandDepth?: number, minScore?: number }} [options]
         * @returns {Promise<Array<{id: number, score: number, payload: any}>>}
         */
        search: (vector, options = {}) =>
            safeInvoke('trivium_search', {
                namespace,
                dim,
                vector,
                topK: options.topK,
                expandDepth: options.expandDepth,
                minScore: options.minScore,
            }),

        /**
         * 认知管线高级检索（FISTA + PPR + DPP）
         * @param {number[]} vector 查询向量
         * @param {object} config 管线配置
         * @returns {Promise<Array<{id: number, score: number, payload: any}>>}
         */
        searchAdvanced: (vector, config = {}) =>
            safeInvoke('trivium_search_advanced', { namespace, dim, vector, config }),

        // ── 文本索引 ──

        /**
         * 为节点建立 BM25 文本索引
         * @param {number} id 节点 ID
         * @param {string} text 文本内容
         */
        indexText: (id, text) =>
            safeInvoke('trivium_index_text', { namespace, dim, id, text }),

        /**
         * 为节点建立 AC 自动机精确匹配关键词索引
         * @param {number} id 节点 ID
         * @param {string} keyword 关键词
         */
        indexKeyword: (id, keyword) =>
            safeInvoke('trivium_index_keyword', { namespace, dim, id, keyword }),

        /**
         * 编译文本索引（批量操作后需要调用一次）
         */
        buildTextIndex: () =>
            safeInvoke('trivium_build_text_index', { namespace, dim }),

        // ── 管理 ──

        /**
         * 手动持久化数据
         */
        flush: () =>
            safeInvoke('trivium_flush', { namespace }),

        /**
         * 关闭数据库实例
         */
        close: () =>
            safeInvoke('trivium_close', { namespace }),

        /**
         * 获取数据库统计信息
         * @returns {Promise<{namespace: string, dim: number, nodeCount: number, estimatedMemoryBytes: number}>}
         */
        stats: () =>
            safeInvoke('trivium_stats', { namespace, dim }),
    };
}

/**
 * @param {{ safeInvoke: (command: any, args?: any) => Promise<any> }} deps
 */
function createDbApi({ safeInvoke }) { // eslint-disable-line jsdoc/check-param-names
    return {
        /**
         * 打开一个 TriviumDB 命名空间
         *
         * @param {string} namespace 命名空间名称（只允许字母、数字、短划线和下划线）
         * @param {{ dim?: number }} [options] 选项（dim 默认 1536）
         * @returns 数据库操作句柄
         */
        open(namespace, options = {}) {
            const dim = options.dim || 1536;
            return createDbHandle({ safeInvoke, namespace, dim });
        },

        /**
         * 列出所有已打开的命名空间
         * @returns {Promise<string[]>}
         */
        listNamespaces() {
            return safeInvoke('trivium_list_namespaces');
        },
    };
}

/**
 * @param {any} context
 */
export function installDbApi(context) {
    const hostWindow = /** @type {any} */ (window);
    const hostAbi = hostWindow.__TAURITAVERN__;
    if (!hostAbi || typeof hostAbi !== 'object') {
        throw new Error('Host ABI __TAURITAVERN__ is missing');
    }

    const safeInvoke = context?.safeInvoke;
    if (typeof safeInvoke !== 'function') {
        throw new Error('Tauri main context safeInvoke is missing');
    }

    if (!hostAbi.api || typeof hostAbi.api !== 'object') {
        hostAbi.api = {};
    }

    hostAbi.api.db = createDbApi({ safeInvoke });
}
