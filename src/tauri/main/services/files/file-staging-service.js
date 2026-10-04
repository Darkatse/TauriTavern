// @ts-check

import { encodeIpcByteBody } from './ipc-byte-body.js';

/**
 * @typedef {import('../../context/types.js').TauriInvokeCommand} TauriInvokeCommand
 * @typedef {(command: TauriInvokeCommand, args?: any, options?: { headers?: HeadersInit }) => Promise<any>} InvokeFn
 */

/** @param {{ safeInvoke?: InvokeFn | undefined; invoke?: InvokeFn | undefined }} [deps] */
export function createFileStagingService({ safeInvoke, invoke } = {}) {
    function rawInvoke() {
        const invokeApi = invoke ?? window.__TAURI__?.core?.invoke;
        if (typeof invokeApi !== 'function') throw new Error('Tauri invoke API is unavailable');
        return invokeApi;
    }

    /** @param {TauriInvokeCommand} command @param {any} args */
    function invokeCommand(command, args) {
        return (safeInvoke ?? rawInvoke())(command, args);
    }

    /** @param {string} filePath */
    async function discardFile(filePath) {
        try {
            await invokeCommand('stage_file_discard', { filePath });
        } catch (error) {
            console.warn('Failed to discard staged file:', error);
        }
    }

    /**
     * @param {Blob} blob
     * @param {{ kind?: string; preferredName?: string; preferredExtension?: string }} [options]
     * @returns {Promise<string>}
     */
    async function stageBlob(blob, { kind = 'generic', preferredName = '', preferredExtension = '' } = {}) {
        const extension = resolveExtension(preferredExtension, preferredName, blob instanceof File ? blob.name : '');
        let filePath = '';
        try {
            const begin = await invokeCommand('stage_file_begin', {
                dto: { kind, preferred_extension: extension, size: blob.size },
            });
            filePath = String(begin.file_path);
            const chunkSize = Number(begin.chunk_size);
            if (!Number.isSafeInteger(chunkSize) || chunkSize <= 0) {
                throw new Error('Host file staging returned an invalid chunk size');
            }
            let offset = 0;
            while (offset < blob.size) {
                const end = Math.min(offset + chunkSize, blob.size);
                const bytes = new Uint8Array(await blob.slice(offset, end).arrayBuffer());
                const { body, headers } = encodeIpcByteBody(bytes);
                const nextOffset = await rawInvoke()('stage_file_chunk', body, {
                    headers: { ...headers, 'file-path': encodeURIComponent(filePath), offset: String(offset) },
                });
                if (Number(nextOffset) !== end) {
                    throw new Error(`Host file staging returned unexpected offset ${nextOffset}`);
                }
                offset = end;
                if (offset < blob.size) await new Promise(resolve => setTimeout(resolve, 0));
            }
            await invokeCommand('stage_file_finish', { filePath, expectedSize: blob.size });
            return filePath;
        } catch (error) {
            if (filePath) await discardFile(filePath);
            throw error;
        }
    }

    return { stageBlob, discardFile };
}

/** @param {string[]} candidates */
function resolveExtension(...candidates) {
    for (const candidate of candidates) {
        const extension = candidate.trim().toLowerCase().replace(/^\./, '').split('.').pop();
        if (extension && /^[a-z0-9]{1,12}$/.test(extension)) return extension;
    }
    return 'bin';
}
