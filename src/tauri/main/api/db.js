// @ts-check

import { registerLifecycleFlushHandler } from '../services/lifecycle/lifecycle-flush-service.js';

/**
 * @typedef {import('./db-types').DatabaseApi} DatabaseApi
 * @typedef {import('./db-types').DatabaseHandle} DatabaseHandle
 * @typedef {{ safeInvoke: (command: string, args?: object) => Promise<any> }} Transport
 */

/** @param {Transport} transport @returns {DatabaseApi} */
export function createDbApi({ safeInvoke }) {
    return {
        async open(namespace, options = {}) {
            const opened = await safeInvoke('database_handle', {
                request: { type: 'open', namespace, options },
            });
            /** @param {Record<string, unknown>} operation */
            const call = operation => safeInvoke('database_handle', {
                request: { type: 'execute', namespace, operation },
            });

            /** @type {DatabaseHandle} */
            const handle = {
                namespace: opened.namespace,
                dim: opened.dim,
                options: Object.freeze(opened.options),
                insert: (vector, payload = null) => call({ type: 'insert', vector, payload }),
                batchInsert: (vectors, payloads) => call({ type: 'batchInsert', vectors, payloads }),
                upsert: (id, vector, payload = null) => call({ type: 'upsert', id, vector, payload }),
                get: id => call({ type: 'get', id }),
                updatePayload: (id, payload) => call({ type: 'updatePayload', id, payload }),
                patchPayload: (id, patch) => call({ type: 'patchPayload', id, patch }),
                updateVector: (id, vector) => call({ type: 'updateVector', id, vector }),
                delete: id => call({ type: 'delete', id }),
                link: (src, dst, label = 'related', weight = 1) => call({ type: 'link', src, dst, label, weight }),
                unlink: (src, dst) => call({ type: 'unlink', src, dst }),
                shortestPath: (source, target, options = {}) => call({ type: 'shortestPath', source, target, ...options }),
                subgraph: (id, options = {}) => call({ type: 'subgraph', id, ...options }),
                indexText: (id, text) => call({ type: 'indexText', id, text }),
                indexKeyword: (id, keyword) => call({ type: 'indexKeyword', id, keyword }),
                buildTextIndex: () => call({ type: 'buildTextIndex' }),
                search: (vector = null, options = {}) => {
                    const { queryText, filter, ...config } = options;
                    return call({ type: 'search', vector, queryText, config: {
                        ...config,
                        payloadFilter: config.payloadFilter ?? filter,
                    } });
                },
                searchBatch: (vectors, options = {}) => {
                    const { parallelism, ...config } = options;
                    return call({ type: 'searchBatch', vectors, parallelism, config });
                },
                searchAdvanced: (vector = null, options = {}) => {
                    const { queryText, ...config } = options;
                    return call({ type: 'searchAdvanced', vector, queryText, config });
                },
                query: (query, params = {}) => call({ type: 'query', query, params }),
                buildQuiverIndex: () => call({ type: 'buildQuiverIndex' }),
                compact: () => call({ type: 'compact' }),
                flush: () => call({ type: 'flush' }),
                close: () => safeInvoke('database_handle', { request: { type: 'close', namespace } }),
                stats: () => call({ type: 'stats' }),
            };
            return Object.freeze(handle);
        },
        listNamespaces: () => safeInvoke('database_handle', { request: { type: 'listNamespaces' } }),
    };
}

/** @param {Transport} context */
export function installDbApi(context) {
    const host = window.__TAURITAVERN__;
    if (!host) throw new Error('TauriTavern host ABI is not installed');
    host.api ??= {};
    host.api.db = createDbApi(context);
    registerLifecycleFlushHandler('databases', () => context.safeInvoke('database_handle', {
        request: { type: 'flushAll' },
    }), { priority: 110 });
}
