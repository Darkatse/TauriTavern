// @ts-check

import { emitLlmConnectionsChanged } from '../../../scripts/tauritavern/agent/llm-connection-events.js';

/**
 * @param {unknown} value
 * @param {string} label
 * @returns {Record<string, any>}
 */
function requirePlainObject(value, label) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(`${label} must be an object`);
    }
    return /** @type {Record<string, any>} */ (value);
}

/**
 * @param {unknown} value
 * @param {string} label
 * @returns {string}
 */
function requireNonEmptyString(value, label) {
    const text = String(value || '').trim();
    if (!text) {
        throw new Error(`${label} is required`);
    }
    return text;
}

/**
 * @param {{ safeInvoke: (command: string, args?: any) => Promise<any> }} deps
 */
function createLlmConnectionsApi({ safeInvoke }) {
    async function list() {
        return safeInvoke('list_llm_connections');
    }

    async function load(input) {
        const connectionId = requireNonEmptyString(input?.connectionId ?? input?.connection_id ?? input, 'connectionId');
        return safeInvoke('load_llm_connection', {
            dto: {
                connectionId,
            },
        });
    }

    async function save(input) {
        const connection = requirePlainObject(input?.connection ?? input, 'connection');
        const result = await safeInvoke('save_llm_connection', {
            dto: {
                connection,
            },
        });
        emitLlmConnectionsChanged();
        return result;
    }

    async function deleteConnection(input) {
        const connectionId = requireNonEmptyString(input?.connectionId ?? input?.connection_id ?? input, 'connectionId');
        const result = await safeInvoke('delete_llm_connection', {
            dto: {
                connectionId,
            },
        });
        emitLlmConnectionsChanged();
        return result;
    }

    return {
        list,
        load,
        save,
        delete: deleteConnection,
    };
}

/**
 * @param {any} context
 */
export function installLlmConnectionsApi(context) {
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

    hostAbi.api.llmConnections = createLlmConnectionsApi({ safeInvoke });
}
