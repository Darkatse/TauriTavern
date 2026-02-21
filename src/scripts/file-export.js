const INVALID_FILENAME_CHARS = /[\\/:*?"<>|]+/g;
const TRAILING_DOTS_OR_SPACES = /[. ]+$/g;
const DEFAULT_FALLBACK_FILE_NAME = 'download.bin';
const FS_WRITE_CHUNK_BYTES = 4 * 1024 * 1024;
const BASE_DIRECTORY_IDS = Object.freeze({
    Document: 6,
    Download: 7,
});
const ANDROID_APP_SCOPED_DIR_PATTERN = /\/android\/data\/[^/]+\/files(\/|$)/i;

function getTauriObject() {
    if (typeof window === 'undefined' || typeof window.__TAURI__ !== 'object') {
        return null;
    }

    return window.__TAURI__;
}

function getPathApi() {
    const pathApi = getTauriObject()?.path;
    if (!pathApi || typeof pathApi.downloadDir !== 'function' || typeof pathApi.join !== 'function') {
        throw new Error('Tauri path API is unavailable');
    }

    return pathApi;
}

function getInvokeApi() {
    const invokeApi = getTauriObject()?.core?.invoke;
    if (typeof invokeApi !== 'function') {
        throw new Error('Tauri invoke API is unavailable');
    }

    return invokeApi;
}

function toUint8Array(value) {
    if (value instanceof Uint8Array) {
        return value;
    }

    if (value instanceof ArrayBuffer) {
        return new Uint8Array(value);
    }

    if (ArrayBuffer.isView(value)) {
        return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    }

    throw new Error('Unsupported binary chunk type');
}

async function fsWriteFileChunk(invokeApi, path, bytes, { append, baseDir }) {
    const writeOptions = { append, create: true };
    if (typeof baseDir === 'number') {
        writeOptions.baseDir = baseDir;
    }

    await invokeApi('plugin:fs|write_file', bytes, {
        headers: {
            path: encodeURIComponent(path),
            options: JSON.stringify(writeOptions),
        },
    });
}

async function writeReadableStreamToPath(invokeApi, path, stream, { baseDir } = {}) {
    if (!stream || typeof stream.getReader !== 'function') {
        throw new Error('Readable stream is required');
    }

    const reader = stream.getReader();
    let append = false;
    let hasWritten = false;

    try {
        while (true) {
            const { done, value } = await reader.read();
            if (done) {
                break;
            }

            const bytes = toUint8Array(value);
            if (bytes.byteLength === 0) {
                continue;
            }

            await fsWriteFileChunk(invokeApi, path, bytes, { append, baseDir });
            append = true;
            hasWritten = true;
        }
    } finally {
        try {
            reader.releaseLock();
        } catch {
            // noop
        }
    }

    if (!hasWritten) {
        await fsWriteFileChunk(invokeApi, path, new Uint8Array(0), { append: false, baseDir });
    }
}

async function writeBlobToPath(invokeApi, path, blob, { baseDir } = {}) {
    if (!(blob instanceof Blob)) {
        throw new Error('Blob payload is required');
    }

    if (blob.size === 0) {
        await fsWriteFileChunk(invokeApi, path, new Uint8Array(0), { append: false, baseDir });
        return;
    }

    let append = false;
    let offset = 0;

    while (offset < blob.size) {
        const end = Math.min(offset + FS_WRITE_CHUNK_BYTES, blob.size);
        const chunk = blob.slice(offset, end);
        const bytes = new Uint8Array(await chunk.arrayBuffer());
        await fsWriteFileChunk(invokeApi, path, bytes, { append, baseDir });
        append = true;
        offset = end;
    }
}

function resolveBaseDirectoryId(pathApi, key, fallbackValue) {
    const baseDirectory = pathApi?.BaseDirectory;
    const value = baseDirectory?.[key];
    return Number.isInteger(value) ? value : fallbackValue;
}

function normalizePathForComparison(value) {
    return String(value || '').replace(/[\\]+/g, '/').replace(/\/+$/, '').toLowerCase();
}

function isAndroidAppScopedDirectory(path) {
    return ANDROID_APP_SCOPED_DIR_PATTERN.test(normalizePathForComparison(path));
}

function isAndroidRuntime() {
    if (typeof navigator === 'undefined' || typeof navigator.userAgent !== 'string') {
        return false;
    }

    return /android/i.test(navigator.userAgent);
}

async function resolveAndroidPublicDownloadDirectory(pathApi) {
    if (!isAndroidRuntime() || typeof pathApi.homeDir !== 'function' || typeof pathApi.join !== 'function') {
        return null;
    }

    try {
        const homeDir = await pathApi.homeDir();
        if (typeof homeDir !== 'string' || !homeDir.trim()) {
            return null;
        }

        const directory = await pathApi.join(homeDir, 'Download');
        if (typeof directory !== 'string' || !directory.trim()) {
            return null;
        }

        return {
            directory,
            baseDir: null,
        };
    } catch {
        return null;
    }
}

async function resolveDownloadDirectory(pathApi) {
    const candidates = [
        typeof pathApi.downloadDir === 'function'
            ? {
                resolver: () => pathApi.downloadDir(),
                baseDir: resolveBaseDirectoryId(pathApi, 'Download', BASE_DIRECTORY_IDS.Download),
            }
            : null,
        typeof pathApi.documentDir === 'function'
            ? {
                resolver: () => pathApi.documentDir(),
                baseDir: resolveBaseDirectoryId(pathApi, 'Document', BASE_DIRECTORY_IDS.Document),
            }
            : null,
    ].filter(Boolean);

    let lastError = null;
    for (const candidate of candidates) {
        try {
            const directory = await candidate.resolver();
            if (typeof directory === 'string' && directory.trim()) {
                return {
                    directory,
                    baseDir: candidate.baseDir,
                };
            }
        } catch (error) {
            lastError = error;
        }
    }

    if (lastError) {
        throw lastError;
    }

    throw new Error('Unable to resolve a writable download directory');
}

async function buildDownloadTarget(pathApi, fileName, fallbackName, { directory, baseDir }) {
    const normalizedName = sanitizeDownloadFileName(fileName, fallbackName);
    const absolutePath = typeof baseDir === 'number'
        ? pathApi.join(directory, normalizedName)
        : directory.replace(/[\\/]+$/, '') + '/' + normalizedName;

    return {
        absolutePath: await absolutePath,
        relativePath: typeof baseDir === 'number' ? normalizedName : absolutePath,
        baseDir,
    };
}

async function resolveMobileDownloadTarget(pathApi, fileName, fallbackName) {
    const directory = await resolveDownloadDirectory(pathApi);
    return buildDownloadTarget(pathApi, fileName, fallbackName, directory);
}

async function resolveMobileBlobDownloadTargets(pathApi, fileName, fallbackName) {
    const scopedDirectory = await resolveDownloadDirectory(pathApi);
    const targets = [await buildDownloadTarget(pathApi, fileName, fallbackName, scopedDirectory)];

    const publicDirectory = await resolveAndroidPublicDownloadDirectory(pathApi);
    if (!publicDirectory || !isAndroidAppScopedDirectory(scopedDirectory.directory)) {
        return targets;
    }

    if (normalizePathForComparison(publicDirectory.directory) === normalizePathForComparison(scopedDirectory.directory)) {
        return targets;
    }

    return [await buildDownloadTarget(pathApi, fileName, fallbackName, publicDirectory), ...targets];
}

function isTauriRuntime() {
    return typeof getTauriObject()?.core?.invoke === 'function';
}

function isMobileRuntime() {
    if (typeof navigator === 'undefined' || typeof navigator.userAgent !== 'string') {
        return false;
    }

    return /android|iphone|ipad|ipod/i.test(navigator.userAgent);
}

export function isNativeMobileDownloadRuntime() {
    return isTauriRuntime() && isMobileRuntime();
}

function sanitizeDownloadFileName(value, fallback = DEFAULT_FALLBACK_FILE_NAME) {
    const fallbackName = String(fallback || DEFAULT_FALLBACK_FILE_NAME).trim() || DEFAULT_FALLBACK_FILE_NAME;
    const rawName = String(value || '').trim();
    const candidate = (rawName || fallbackName)
        .replace(INVALID_FILENAME_CHARS, '_')
        .replace(TRAILING_DOTS_OR_SPACES, '')
        .trim();

    return candidate || fallbackName;
}

export async function writeReadableStreamToMobileDownloadFolder(stream, fileName, options = {}) {
    if (!stream) {
        throw new Error('Readable stream is required');
    }

    const pathApi = getPathApi();
    const invokeApi = getInvokeApi();
    const target = await resolveMobileDownloadTarget(pathApi, fileName, options.fallbackName);

    await writeReadableStreamToPath(invokeApi, target.relativePath, stream, {
        baseDir: target.baseDir,
    });

    return target.absolutePath;
}

async function writeBlobToMobileDownloadFolder(blob, fileName, options = {}) {
    const pathApi = getPathApi();
    const invokeApi = getInvokeApi();
    const targets = await resolveMobileBlobDownloadTargets(pathApi, fileName, options.fallbackName);

    let lastError = null;
    for (const target of targets) {
        try {
            await writeBlobToPath(invokeApi, target.relativePath, blob, {
                baseDir: target.baseDir,
            });
            return target.absolutePath;
        } catch (error) {
            lastError = error;
            console.warn(`Blob export write attempt failed at ${target.absolutePath}:`, error);
        }
    }

    if (lastError) {
        throw lastError;
    }

    throw new Error('Unable to write blob to mobile download directory');
}

function triggerBrowserDownload(blob, fileName, { fallbackName = DEFAULT_FALLBACK_FILE_NAME } = {}) {
    const payload = blob instanceof Blob ? blob : new Blob([blob ?? '']);
    const normalizedName = sanitizeDownloadFileName(fileName, fallbackName);
    const objectUrl = URL.createObjectURL(payload);
    const anchor = document.createElement('a');

    anchor.href = objectUrl;
    anchor.download = normalizedName;
    document.body.append(anchor);
    anchor.click();
    anchor.remove();

    // Let the browser begin the download before releasing the object URL.
    setTimeout(() => URL.revokeObjectURL(objectUrl), 0);
}

export async function downloadBlobWithRuntime(
    blob,
    fileName,
    {
        fallbackName = DEFAULT_FALLBACK_FILE_NAME,
    } = {},
) {
    const payload = blob instanceof Blob ? blob : new Blob([blob ?? '']);

    if (isNativeMobileDownloadRuntime()) {
        const savedPath = await writeBlobToMobileDownloadFolder(payload, fileName, { fallbackName });
        return { mode: 'mobile-native', savedPath };
    }

    triggerBrowserDownload(payload, fileName, { fallbackName });
    return { mode: 'browser', savedPath: '' };
}
