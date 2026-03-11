// @ts-check

/**
 * @typedef {import('../../context/types.js').MaterializedFileInfo} MaterializedFileInfo
 */

export function createUploadService() {
    const UPLOAD_CHUNK_BYTES = 4 * 1024 * 1024;

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

    /** @param {string | null} filePath */
    function shouldUseDirectUploadPath(filePath) {
        if (!isLikelyFileSystemPath(filePath)) {
            return false;
        }

        // Android file pickers often expose content URIs or external paths that are not directly readable by Rust.
        // Materializing into app storage keeps behavior consistent and permission-safe.
        if (isAndroidRuntime()) {
            return false;
        }

        return true;
    }

    /** @param {any} pathApi */
    async function resolveUploadDirectory(pathApi) {
        const getAppCacheDir = typeof pathApi?.appCacheDir === 'function'
            ? () => pathApi.appCacheDir()
            : null;
        const getTempDir = typeof pathApi?.tempDir === 'function'
            ? () => pathApi.tempDir()
            : null;

        const candidates = [];
        if (isAndroidRuntime() && getAppCacheDir) {
            candidates.push(getAppCacheDir);
        }

        if (getTempDir) {
            candidates.push(getTempDir);
        }

        if (getAppCacheDir && !candidates.includes(getAppCacheDir)) {
            candidates.push(getAppCacheDir);
        }

        let lastError = null;
        for (const candidate of candidates) {
            try {
                const directory = await candidate();
                if (typeof directory === 'string' && directory.trim()) {
                    return directory;
                }
            } catch (error) {
                lastError = error;
            }
        }

        if (lastError) {
            throw lastError;
        }

        throw new Error('No writable upload directory is available');
    }

    /** @param {string} filePath @param {Blob} file @param {Function} invokeApi */
    async function writeTempUploadFile(filePath, file, invokeApi) {
        await writeTempUploadFileChunked(filePath, file, invokeApi);
    }

    /** @param {string} filePath @param {Blob} file @param {Function} invokeApi */
    async function writeTempUploadFileChunked(filePath, file, invokeApi) {
        let offset = 0;
        let append = false;

        if (file.size === 0) {
            await invokeApi('plugin:fs|write_file', new Uint8Array(0), {
                headers: {
                    path: encodeURIComponent(filePath),
                    options: JSON.stringify({ append: false, create: true }),
                },
            });
            return;
        }

        while (offset < file.size) {
            const end = Math.min(offset + UPLOAD_CHUNK_BYTES, file.size);
            const chunk = file.slice(offset, end);
            const bytes = new Uint8Array(await chunk.arrayBuffer());

            await invokeApi('plugin:fs|write_file', bytes, {
                headers: {
                    path: encodeURIComponent(filePath),
                    options: JSON.stringify({ append, create: true }),
                },
            });

            offset = end;
            append = true;
        }
    }

    /** @param {string} filePath @param {Function} invokeApi */
    async function removeTempUploadFile(filePath, invokeApi) {
        await invokeApi('plugin:fs|remove', { path: filePath });
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
     * @param {{ preferredName?: string; preferredExtension?: string } | undefined} options
     * @returns {Promise<MaterializedFileInfo | null>}
     */
    async function materializeUploadFile(file, { preferredName = '', preferredExtension = '' } = {}) {
        if (!(file instanceof Blob)) {
            return null;
        }

        const directPath = extractNativeFilePath(file);
        if (shouldUseDirectUploadPath(directPath)) {
            return {
                filePath: /** @type {string} */ (directPath),
                isTemporary: false,
            };
        }

        const tauri = window.__TAURI__;
        const pathApi = tauri?.path;
        const invokeApi = tauri?.core?.invoke;
        if (typeof pathApi?.join !== 'function') {
            return {
                filePath: '',
                error: 'Tauri path API is unavailable',
            };
        }

        if (typeof invokeApi !== 'function') {
            return {
                filePath: '',
                error: 'Tauri invoke API is unavailable',
            };
        }

        const extension = resolveUploadExtension({
            preferredExtension,
            preferredName,
            sourceName: file instanceof File ? file.name : '',
        });
        const fileName = `tauritavern-upload-${Date.now()}-${Math.random().toString(16).slice(2)}.${extension}`;

        try {
            const uploadDir = await resolveUploadDirectory(pathApi);
            const filePath = await pathApi.join(uploadDir, fileName);
            await writeTempUploadFile(filePath, file, invokeApi);

            return {
                filePath,
                isTemporary: true,
                cleanup: async () => {
                    try {
                        await removeTempUploadFile(filePath, invokeApi);
                    } catch {
                        // noop
                    }
                },
            };
        } catch (error) {
            console.warn('Tauri temp file write failed:', error);
            return {
                filePath: '',
                // @ts-ignore - normalize unknown error shape.
                error: error?.message || 'Failed to materialize upload file',
                isTemporary: false,
            };
        }
    }

    return {
        materializeUploadFile,
        removeTempUploadFile,
        isAndroidRuntime,
    };
}
