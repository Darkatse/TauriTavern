// @ts-check

import { decodeBase64ToBytes, encodeBytesToBase64 } from '../binary-utils.js';
import { textFragmentsToByteChunks } from '../kernel/utf8.js';
import { commitBytes } from '../services/files/byte-commit.js';
import { createReadableFileStreamService } from '../services/files/readable-file-stream-service.js';

/** @param {unknown} value @param {string} label */
function requireNonEmptyString(value, label) {
    const resolved = String(value || '').trim();
    if (!resolved) throw new Error(`${label} is required`);
    return resolved;
}

/** @param {any} options */
function tableOptions(options) {
    return {
        namespace: requireNonEmptyString(options?.namespace, 'namespace'),
        table: typeof options?.table === 'string' ? options.table.trim() || undefined : undefined,
    };
}

/** @param {any} options */
function entryOptions(options) {
    return { ...tableOptions(options), key: requireNonEmptyString(options?.key, 'key') };
}

/** @param {{ namespace: string; key: string; table?: string }} entry */
function entryLabel(entry) {
    return `${entry.namespace}:${entry.table ?? 'main'}/${entry.key}`;
}

/** @param {Blob} blob @param {number} maxBytes */
async function* blobFrames(blob, maxBytes) {
    for (let offset = 0; offset < blob.size; offset += maxBytes) {
        yield new Uint8Array(await blob.slice(offset, offset + maxBytes).arrayBuffer());
    }
}

/** @param {ReadableStream<Uint8Array>} stream */
async function readJson(stream) {
    const reader = stream.pipeThrough(new TextDecoderStream('utf-8', { fatal: true })).getReader();
    let text = '';
    try {
        for (;;) {
            const { done, value } = await reader.read();
            if (done) break;
            text += value;
        }
    } finally {
        reader.releaseLock();
    }
    return JSON.parse(text);
}

/** @param {{ safeInvoke: Function; invokeTransport: Function }} deps */
function createExtensionStoreApi({ safeInvoke, invokeTransport }) {
    const { createByteStream } = createReadableFileStreamService({ invoke: invokeTransport });

    async function openEntry(entry, kind) {
        const opened = await invokeTransport('open_extension_store_entry', { ...entry, kind });
        if (opened === null) return null;
        return { stream: createByteStream(opened.readerId), mimeType: opened.mimeType };
    }

    function commit(entry, operation, frames) {
        return commitBytes({
            begin: () => invokeTransport('begin_extension_store_commit', { ...entry, operation }),
            frames,
            append: (body, options) => invokeTransport(
                'append_extension_store_commit_chunk', body, options,
            ),
            finish: (sessionId, expectedSize) => invokeTransport('finish_extension_store_commit', {
                sessionId,
                expectedSize,
            }),
            abort: sessionId => invokeTransport('abort_extension_store_commit', { sessionId }),
        });
    }

    async function tryGetJson(options) {
        const entry = entryOptions(options);
        const opened = await openEntry(entry, 'json');
        if (opened === null) return { found: false };
        try {
            return { found: true, value: await readJson(opened.stream) };
        } catch (error) {
            throw new Error(`Failed to read extension store JSON ${entryLabel(entry)}: ${error.message}`, {
                cause: error,
            });
        }
    }

    async function getJson(options) {
        const result = await tryGetJson(options);
        if (!result.found) {
            throw new Error(`Extension store entry not found: ${entryLabel(entryOptions(options))}`);
        }
        return result.value;
    }

    function writeJson(options, operation) {
        const entry = entryOptions(options);
        // Capture before begin() yields: later caller edits belong to a later commit.
        const text = JSON.stringify(options?.value);
        if (typeof text !== 'string') throw new Error('Extension store value is not JSON serializable');
        return commit(entry, operation, maxBytes => textFragmentsToByteChunks([text], maxBytes));
    }

    async function setJson(options) {
        return writeJson(options, 'setJson');
    }

    async function updateJson(options) {
        return writeJson(options, 'updateJson');
    }

    async function setBlob(options) {
        const entry = entryOptions(options);
        const data = options?.data;
        let snapshot;
        if (typeof data === 'string') {
            const text = data.trim();
            const bytes = decodeBase64ToBytes(text);
            if (encodeBytesToBase64(bytes) !== text) {
                throw new Error('Invalid extension store base64 data');
            }
            snapshot = new Blob([bytes]);
        } else if (data instanceof Blob) {
            snapshot = data;
        } else if (data instanceof ArrayBuffer || ArrayBuffer.isView(data)) {
            // Blob snapshots mutable buffers now and supplies immutable slices during upload.
            snapshot = new Blob([data]);
        } else {
            throw new Error('Extension store data must be a Blob, byte buffer, or base64 string');
        }
        return commit(entry, 'setBlob', maxBytes => blobFrames(snapshot, maxBytes));
    }

    async function openBlob(options) {
        const entry = entryOptions(options);
        const opened = await openEntry(entry, 'blob');
        if (opened === null) throw new Error(`Extension store entry not found: ${entryLabel(entry)}`);
        return opened;
    }

    async function getBlob(options) {
        const { stream, mimeType } = await openBlob(options);
        return new Response(stream, { headers: { 'Content-Type': mimeType } }).blob();
    }

    async function getBlobStream(options) {
        return (await openBlob(options)).stream;
    }

    async function renameKey(options) {
        return safeInvoke('rename_extension_store_key', {
            ...entryOptions(options),
            newKey: requireNonEmptyString(options?.newKey, 'newKey'),
        });
    }

    async function deleteJson(options) {
        return safeInvoke('delete_extension_store_entry', { ...entryOptions(options), kind: 'json' });
    }

    async function deleteBlob(options) {
        return safeInvoke('delete_extension_store_entry', { ...entryOptions(options), kind: 'blob' });
    }

    async function listKeys(options) {
        return safeInvoke('list_extension_store_keys', { ...tableOptions(options), kind: 'json' });
    }

    async function listBlobKeys(options) {
        return safeInvoke('list_extension_store_keys', { ...tableOptions(options), kind: 'blob' });
    }

    async function listTables(options) {
        return safeInvoke('list_extension_store_tables', {
            namespace: requireNonEmptyString(options?.namespace, 'namespace'),
        });
    }

    async function deleteTable(options) {
        return safeInvoke('delete_extension_store_table', {
            namespace: requireNonEmptyString(options?.namespace, 'namespace'),
            table: requireNonEmptyString(options?.table, 'table'),
        });
    }

    return {
        getJson,
        tryGetJson,
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
        getBlobStream,
        setBlob,
        deleteBlob,
        listBlobKeys,
    };
}

/** @param {any} context */
export function installExtensionStoreApi(context) {
    const hostWindow = /** @type {any} */ (window);
    const hostAbi = hostWindow.__TAURITAVERN__;
    if (!hostAbi || typeof hostAbi !== 'object') {
        throw new Error('Host ABI __TAURITAVERN__ is missing');
    }
    const { safeInvoke, invokeTransport } = context ?? {};
    if (typeof safeInvoke !== 'function' || typeof invokeTransport !== 'function') {
        throw new Error('Tauri main invoke transport is missing');
    }
    hostAbi.api ??= {};
    hostAbi.api.extension ??= {};
    hostAbi.api.extension.store = createExtensionStoreApi({ safeInvoke, invokeTransport });
}
