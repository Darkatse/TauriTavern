// @ts-check

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

    function isAndroidRuntime() {
        if (typeof navigator === 'undefined' || typeof navigator.userAgent !== 'string') {
            return false;
        }

        return /android/i.test(navigator.userAgent);
    }

    function isIosRuntime() {
        if (typeof navigator === 'undefined') {
            return false;
        }

        const userAgent = typeof navigator.userAgent === 'string' ? navigator.userAgent : '';
        if (/iphone|ipad|ipod/i.test(userAgent)) {
            return true;
        }

        const touchPoints = Number(navigator.maxTouchPoints || 0);
        if (touchPoints <= 1) {
            return false;
        }

        const platform = typeof navigator.platform === 'string' ? navigator.platform : '';
        return platform === 'MacIntel' || /macintosh/i.test(userAgent);
    }

    function isMobileUploadRuntime() {
        return isAndroidRuntime() || isIosRuntime();
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

    /** @param {Uint8Array} bytes */
    function bytesToBase64(bytes) {
        if (typeof btoa !== 'function') {
            throw new Error('Base64 encoder is unavailable');
        }

        let binary = '';
        const stride = 0x8000;
        for (let offset = 0; offset < bytes.byteLength; offset += stride) {
            const chunk = bytes.subarray(offset, offset + stride);
            binary += String.fromCharCode(...chunk);
        }

        return btoa(binary);
    }

    /** @param {Blob} blob */
    async function blobToBase64(blob) {
        if (typeof FileReader === 'function') {
            return new Promise((resolve, reject) => {
                const reader = new FileReader();
                reader.onerror = () => reject(reader.error || new Error('Failed to read upload chunk'));
                reader.onload = () => {
                    const result = typeof reader.result === 'string' ? reader.result : '';
                    const separator = result.indexOf(',');
                    if (separator < 0) {
                        reject(new Error('Upload chunk data URL is invalid'));
                        return;
                    }

                    resolve(result.slice(separator + 1));
                };
                reader.readAsDataURL(blob);
            });
        }

        return bytesToBase64(new Uint8Array(await blob.arrayBuffer()));
    }

    /** @param {string | null} filePath */
    function shouldUseDirectUploadPath(filePath) {
        if (!isLikelyFileSystemPath(filePath)) {
            return false;
        }

        // Mobile file pickers often expose scoped paths that are not directly readable by Rust.
        // Materializing into app storage keeps behavior consistent and permission-safe.
        if (isMobileUploadRuntime()) {
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

        if (isAndroidRuntime()) {
            return invokeApi('stage_upload_chunk', { data: await blobToBase64(chunk) }, {
                headers: {
                    'chunk-encoding': 'base64',
                    'file-path': encodeURIComponent(filePath),
                    offset: String(offset),
                },
            });
        }

        const bytes = new Uint8Array(await chunk.arrayBuffer());
        return invokeApi('stage_upload_chunk', bytes, {
            headers: {
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

    /** @param {string} platform */
    function nativeArchivePickerError(platform) {
        return `${platform} data archive imports must use the native archive picker`;
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
        if (uploadKind === DATA_ARCHIVE_KIND) {
            if (isAndroidRuntime()) {
                return {
                    filePath: '',
                    error: nativeArchivePickerError('Android'),
                    isTemporary: false,
                };
            }

            if (isIosRuntime()) {
                return {
                    filePath: '',
                    error: nativeArchivePickerError('iOS'),
                    isTemporary: false,
                };
            }
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
        isAndroidRuntime,
        isIosRuntime,
    };
}
