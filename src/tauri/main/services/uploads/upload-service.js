// @ts-check

import { isMobileHost } from '../../../../scripts/util/host-identity.js';
import { encodeIpcByteBody } from '../files/ipc-byte-body.js';

/**
 * @typedef {import('../../context/types.js').MaterializedFileInfo} MaterializedFileInfo
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any) => Promise<any>} SafeInvokeFn
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any, options?: { headers?: HeadersInit }) => Promise<any>} RawInvokeFn
 */

/**
 * @param {{ safeInvoke?: SafeInvokeFn; invoke?: RawInvokeFn }} [deps]
 */
export function createUploadService({ safeInvoke, invoke } = {}) {
    const DATA_ARCHIVE_KIND = 'data-archive';
    const DEFAULT_UPLOAD_KIND = 'generic';

    /** @param {any} file */
    function extractNativeFilePath(file) {
        if (!file || typeof file !== 'object') {
            return null;
        }

        // @ts-ignore - non-standard fields provided by WebView file inputs.
        const candidate = file.path || file.webkitRelativePath || null;
        if (!candidate || typeof candidate !== 'string') {
            return null;
        }

        if (candidate.toLowerCase().includes('fakepath')) {
            return null;
        }

        return candidate;
    }

    /** @param {any} value */
    function isLikelyFileSystemPath(value) {
        if (typeof value !== 'string' || !value.trim()) {
            return false;
        }

        const normalized = value.trim();
        if (/^[a-z]+:\/\//i.test(normalized)) {
            return false;
        }

        return (
            normalized.startsWith('/') ||
            normalized.startsWith('\\\\') ||
            /^[a-z]:[\\/]/i.test(normalized)
        );
    }

    /** @param {any} value */
    function normalizeUploadKind(value) {
        const kind = String(value || DEFAULT_UPLOAD_KIND).trim() || DEFAULT_UPLOAD_KIND;
        if (!/^[a-z0-9_-]{1,48}$/.test(kind)) {
            throw new Error(`Invalid upload kind: ${kind}`);
        }

        return kind;
    }

    async function yieldToEventLoop() {
        await new Promise(resolve => setTimeout(resolve, 0));
    }

    /** @param {string | null} filePath */
    function shouldUseDirectUploadPath(filePath) {
        if (!isLikelyFileSystemPath(filePath)) {
            return false;
        }

        // Mobile file pickers often expose scoped paths that are not directly readable by Rust.
        // Materializing into app storage keeps behavior consistent and permission-safe.
        if (isMobileHost()) {
            return false;
        }

        return true;
    }

    /** @param {string} filePath @param {Function} invokeApi */
    async function removeTempUploadFile(filePath, invokeApi) {
        await invokeApi('plugin:fs|remove', { path: filePath });
    }

    /**
     * @param {import('../../context/types.js').TauriInvokeCommand} command
     * @param {any} args
     */
    async function invokeHostUploadCommand(command, args) {
        if (typeof safeInvoke === 'function') {
            return safeInvoke(command, args);
        }

        const invokeApi = window.__TAURI__?.core?.invoke;
        if (typeof invokeApi !== 'function') {
            throw new Error('Tauri invoke API is unavailable');
        }

        return invokeApi(command, args);
    }

    /**
     * @param {string} filePath
     * @param {number} offset
     * @param {Blob} chunk
     */
    async function invokeHostUploadChunk(filePath, offset, chunk) {
        const invokeApi = typeof invoke === 'function'
            ? invoke
            : window.__TAURI__?.core?.invoke;
        if (typeof invokeApi !== 'function') {
            throw new Error('Tauri invoke API is unavailable');
        }

        const bytes = new Uint8Array(await chunk.arrayBuffer());
        const { body, headers } = encodeIpcByteBody(bytes);
        return invokeApi('stage_upload_chunk', body, {
            headers: {
                ...headers,
                'file-path': encodeURIComponent(filePath),
                offset: String(offset),
            },
        });
    }

    /** @param {any} value */
    function normalizeHostChunkSize(value) {
        const chunkSize = Math.floor(Number(value) || 0);
        if (!Number.isSafeInteger(chunkSize) || chunkSize <= 0) {
            throw new Error('Host upload service returned an invalid chunk size');
        }
        return chunkSize;
    }

    /**
     * @param {Blob} file
     * @param {{ kind: string; preferredName: string; preferredExtension: string }} options
     * @returns {Promise<MaterializedFileInfo>}
     */
    async function materializeUploadFileViaHostStaging(file, { kind, preferredName, preferredExtension }) {
        const extension = resolveUploadExtension({
            preferredExtension,
            preferredName,
            sourceName: file instanceof File ? file.name : '',
        });
        let filePath = '';

        try {
            const begin = await invokeHostUploadCommand('stage_upload_begin', {
                dto: {
                    kind,
                    preferred_extension: extension,
                    size: file.size,
                },
            });
            filePath = String(begin?.file_path || '').trim();
            if (!filePath) {
                throw new Error('Host upload service did not return a file path');
            }

            const chunkSize = normalizeHostChunkSize(begin?.chunk_size);
            let offset = 0;
            while (offset < file.size) {
                const end = Math.min(offset + chunkSize, file.size);
                const chunk = file.slice(offset, end);
                const nextOffset = await invokeHostUploadChunk(filePath, offset, chunk);
                offset = Number(nextOffset);
                if (offset !== end) {
                    throw new Error(`Host upload service returned unexpected offset ${nextOffset}`);
                }
                if (offset < file.size) {
                    await yieldToEventLoop();
                }
            }

            const finished = await invokeHostUploadCommand('stage_upload_finish', {
                file_path: filePath,
                expected_size: file.size,
            });
            const finishedPath = String(finished?.file_path || filePath).trim();
            if (!finishedPath) {
                throw new Error('Host upload service did not return a finished file path');
            }

            return {
                filePath: finishedPath,
                isTemporary: true,
                cleanup: async () => {
                    try {
                        await invokeHostUploadCommand('stage_upload_discard', {
                            file_path: finishedPath,
                        });
                    } catch (error) {
                        console.warn('Failed to cleanup staged upload file:', error);
                    }
                },
            };
        } catch (error) {
            if (filePath) {
                try {
                    await invokeHostUploadCommand('stage_upload_discard', {
                        file_path: filePath,
                    });
                } catch {
                    // Cleanup is best-effort after a failed staging attempt.
                }
            }
            throw error;
        }
    }

    /**
     * @param {{ preferredExtension: any; preferredName: any; sourceName: any }} params
     */
    function resolveUploadExtension({ preferredExtension, preferredName, sourceName }) {
        const candidates = [preferredExtension, preferredName, sourceName];

        for (const candidate of candidates) {
            const normalized = normalizeExtensionCandidate(candidate);
            if (normalized) {
                return normalized;
            }
        }

        return 'bin';
    }

    /** @param {any} value */
    function normalizeExtensionCandidate(value) {
        if (typeof value !== 'string' || !value.trim()) {
            return null;
        }

        const cleaned = value.trim().toLowerCase().replace(/^\./, '');
        const extension = cleaned.includes('.') ? cleaned.split('.').pop() : cleaned;
        if (!extension) {
            return null;
        }

        return /^[a-z0-9]{1,12}$/.test(extension) ? extension : null;
    }

    /**
     * @param {Blob} file
     * @param {{ preferredName?: string; preferredExtension?: string; kind?: string } | undefined} options
     * @returns {Promise<MaterializedFileInfo | null>}
     */
    async function materializeUploadFile(file, { preferredName = '', preferredExtension = '', kind = DEFAULT_UPLOAD_KIND } = {}) {
        if (!(file instanceof Blob)) {
            return null;
        }

        const uploadKind = normalizeUploadKind(kind);
        if (uploadKind === DATA_ARCHIVE_KIND && isMobileHost()) {
            return {
                filePath: '',
                error: 'Mobile data archive imports must use the native archive picker',
                isTemporary: false,
            };
        }

        const directPath = extractNativeFilePath(file);
        if (shouldUseDirectUploadPath(directPath)) {
            return {
                filePath: /** @type {string} */ (directPath),
                isTemporary: false,
            };
        }

        try {
            return await materializeUploadFileViaHostStaging(file, {
                kind: uploadKind,
                preferredName,
                preferredExtension,
            });
        } catch (error) {
            console.warn('Tauri host upload staging failed:', error);
            return {
                filePath: '',
                // @ts-ignore - normalize unknown error shape.
                error: error?.message || 'Failed to stage upload file',
                isTemporary: false,
            };
        }
    }

    return {
        materializeUploadFile,
        removeTempUploadFile,
    };
}
