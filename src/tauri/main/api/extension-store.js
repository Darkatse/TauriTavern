// @ts-check

import { decodeBase64ToBytes, encodeBytesToBase64, normalizeBinaryPayload } from '../binary-utils.js';

/**
 * @param {unknown} value
 * @param {string} label
 */
function requireNonEmptyString(value, label) {
    const resolved = String(value || '').trim();
    if (!resolved) {
        throw new Error(`${label} is required`);
    }
    return resolved;
}

/**
 * @param {unknown} value
 * @returns {string | undefined}
 */
function normalizeOptionalName(value) {
    const resolved = typeof value === 'string' ? value.trim() : '';
    return resolved ? resolved : undefined;
}

/**
 * @param {{ safeInvoke: (command: any, args?: any) => Promise<any> }} deps
 */
function createExtensionStoreApi({ safeInvoke }) {
    async function getJson(options) {
        const namespace = requireNonEmptyString(options?.namespace, 'namespace');
        const key = requireNonEmptyString(options?.key, 'key');
        const table = normalizeOptionalName(options?.table);
        return safeInvoke('get_extension_store_json', { namespace, key, table });
    }

    async function setJson(options) {
        const namespace = requireNonEmptyString(options?.namespace, 'namespace');
        const key = requireNonEmptyString(options?.key, 'key');
        const table = normalizeOptionalName(options?.table);
        const value = options?.value;
        return safeInvoke('set_extension_store_json', { namespace, key, value, table });
    }

    async function updateJson(options) {
        const namespace = requireNonEmptyString(options?.namespace, 'namespace');
        const key = requireNonEmptyString(options?.key, 'key');
        const table = normalizeOptionalName(options?.table);
        const value = options?.value;
        return safeInvoke('update_extension_store_json', { namespace, key, value, table });
    }

    async function renameKey(options) {
        const namespace = requireNonEmptyString(options?.namespace, 'namespace');
        const key = requireNonEmptyString(options?.key, 'key');
        const newKey = requireNonEmptyString(options?.newKey, 'newKey');
        const table = normalizeOptionalName(options?.table);
        return safeInvoke('rename_extension_store_key', { namespace, key, newKey, table });
    }

    async function deleteJson(options) {
        const namespace = requireNonEmptyString(options?.namespace, 'namespace');
        const key = requireNonEmptyString(options?.key, 'key');
        const table = normalizeOptionalName(options?.table);
        return safeInvoke('delete_extension_store_json', { namespace, key, table });
    }

    async function listKeys(options) {
        const namespace = requireNonEmptyString(options?.namespace, 'namespace');
        const table = normalizeOptionalName(options?.table);
        return safeInvoke('list_extension_store_keys', { namespace, table });
    }

    async function listTables(options) {
        const namespace = requireNonEmptyString(options?.namespace, 'namespace');
        return safeInvoke('list_extension_store_tables', { namespace });
    }

    async function deleteTable(options) {
        const namespace = requireNonEmptyString(options?.namespace, 'namespace');
        const table = requireNonEmptyString(options?.table, 'table');
        return safeInvoke('delete_extension_store_table', { namespace, table });
    }

    async function setBlob(options) {
        const namespace = requireNonEmptyString(options?.namespace, 'namespace');
        const key = requireNonEmptyString(options?.key, 'key');
        const table = normalizeOptionalName(options?.table);

        const data = options?.data;
        if (data === null || data === undefined) {
            throw new Error('data is required');
        }

        let dataBase64 = '';
        if (typeof data === 'string') {
            dataBase64 = data.trim();
        } else if (data instanceof Blob) {
            const bytes = new Uint8Array(await data.arrayBuffer());
            dataBase64 = encodeBytesToBase64(bytes);
        } else {
            const bytes = normalizeBinaryPayload(data);
            dataBase64 = encodeBytesToBase64(bytes);
        }

        if (!dataBase64) {
            throw new Error('data must not be empty');
        }

        return safeInvoke('set_extension_store_blob', { namespace, key, dataBase64, table });
    }

    async function getBlob(options) {
        const namespace = requireNonEmptyString(options?.namespace, 'namespace');
        const key = requireNonEmptyString(options?.key, 'key');
        const table = normalizeOptionalName(options?.table);

        const payload = await safeInvoke('get_extension_store_blob', { namespace, key, table });
        const base64 = String(payload?.content_base64 || '').trim();
        const mimeType = String(payload?.mime_type || 'application/octet-stream').trim();

        const bytes = decodeBase64ToBytes(base64);
        return new Blob([bytes], { type: mimeType });
    }

    async function deleteBlob(options) {
        const namespace = requireNonEmptyString(options?.namespace, 'namespace');
        const key = requireNonEmptyString(options?.key, 'key');
        const table = normalizeOptionalName(options?.table);
        return safeInvoke('delete_extension_store_blob', { namespace, key, table });
    }

    async function listBlobKeys(options) {
        const namespace = requireNonEmptyString(options?.namespace, 'namespace');
        const table = normalizeOptionalName(options?.table);
        return safeInvoke('list_extension_store_blob_keys', { namespace, table });
    }

    return {
        getJson,
        setJson,
        updateJson,
        updateJSON: updateJson,
        renameKey,
        updateKey: renameKey,
        deleteJson,
        listKeys,
        listTables,
        deleteTable,
        getBlob,
        setBlob,
        deleteBlob,
        listBlobKeys,
    };
}

/**
 * @param {any} context
 */
export function installExtensionStoreApi(context) {
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

    if (!hostAbi.api.extension || typeof hostAbi.api.extension !== 'object') {
        hostAbi.api.extension = {};
    }

    hostAbi.api.extension.store = createExtensionStoreApi({ safeInvoke });
}
