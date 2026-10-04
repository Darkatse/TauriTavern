import { hostPlatform } from './util/host-identity.js';

const INVALID_FILENAME_CHARS = /[\\/:*?"<>|]+/g;
const TRAILING_DOTS_OR_SPACES = /[. ]+$/g;
const DEFAULT_FALLBACK_FILE_NAME = 'download.bin';
const DEFAULT_MIME_TYPE = 'application/octet-stream';
const MIME_TYPE_PATTERN = /^[A-Za-z0-9][A-Za-z0-9!#$&^_.+-]*\/[A-Za-z0-9][A-Za-z0-9!#$&^_.+-]*$/;
const FS_WRITE_CHUNK_BYTES = 4 * 1024 * 1024;
const IOS_EXPORT_STAGING_ROOT_NAME = 'tauritavern-export-staging';
const IOS_EXPORT_STAGING_PREFIX = 'tauritavern-export-';
const ANDROID_EXPORT_STAGING_ROOT_NAME = IOS_EXPORT_STAGING_ROOT_NAME;
const NATIVE_EXPORT_STAGING_ROOT_NAME = IOS_EXPORT_STAGING_ROOT_NAME;
const NATIVE_EXPORT_STAGING_PREFIX = IOS_EXPORT_STAGING_PREFIX;
const ANDROID_PUBLIC_DOWNLOAD_BRIDGE_NAME = 'TauriTavernAndroidPublicDownloadBridge';
const ANDROID_PUBLIC_DOWNLOAD_PICKER_RECEIVER = '__TAURITAVERN_PUBLIC_DOWNLOAD_PICKER__';

let androidPublicDownloadPickerPending = null;

function getTauriObject() {
    if (typeof window === 'undefined' || typeof window.__TAURI__ !== 'object') {
        return null;
    }

    return window.__TAURI__;
}

function getPathApi() {
    const pathApi = getTauriObject()?.path;
    if (!pathApi || typeof pathApi.join !== 'function') {
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

async function fsWriteFileChunk(invokeApi, path, bytes, { append }) {
    const writeOptions = { append, create: true };

    await invokeApi('plugin:fs|write_file', bytes, {
        headers: {
            path: encodeURIComponent(path),
            options: JSON.stringify(writeOptions),
        },
    });
}

async function writeBlobToPath(invokeApi, path, blob) {
    if (!(blob instanceof Blob)) {
        throw new Error('Blob payload is required');
    }

    if (blob.size === 0) {
        await fsWriteFileChunk(invokeApi, path, new Uint8Array(0), { append: false });
        return;
    }

    let append = false;
    let offset = 0;

    while (offset < blob.size) {
        const end = Math.min(offset + FS_WRITE_CHUNK_BYTES, blob.size);
        const chunk = blob.slice(offset, end);
        const bytes = new Uint8Array(await chunk.arrayBuffer());
        await fsWriteFileChunk(invokeApi, path, bytes, { append });
        append = true;
        offset = end;
    }
}

function isTauriRuntime() {
    return typeof getTauriObject()?.core?.invoke === 'function';
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

function createNativeExportStagingDirectoryName() {
    return `${NATIVE_EXPORT_STAGING_PREFIX}${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

async function resolveNativeExportStagingDirectory(pathApi, platformName) {
    const candidates = [
        typeof pathApi.appCacheDir === 'function' ? () => pathApi.appCacheDir() : null,
        typeof pathApi.tempDir === 'function' ? () => pathApi.tempDir() : null,
    ].filter(Boolean);

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

    throw new Error(`No writable ${platformName} export staging directory is available`);
}

async function resolveAndroidExportStagingDirectory(pathApi) {
    if (typeof pathApi.appCacheDir !== 'function') {
        throw new Error('Android app cache directory API is unavailable');
    }

    const directory = await pathApi.appCacheDir();
    if (typeof directory !== 'string' || !directory.trim()) {
        throw new Error('Android app cache directory is unavailable');
    }

    return directory;
}

async function stageBlobForNativeFileBridge(
    blob,
    fileName,
    {
        fallbackName,
        platformName,
        stagingBaseRoot = null,
        stagingRootName = NATIVE_EXPORT_STAGING_ROOT_NAME,
    } = {},
) {
    const pathApi = getPathApi();
    const invokeApi = getInvokeApi();
    const resolvedStagingBaseRoot = stagingBaseRoot
        || await resolveNativeExportStagingDirectory(pathApi, platformName || 'mobile');
    const stagingRoot = await pathApi.join(resolvedStagingBaseRoot, stagingRootName);
    const stagingDirectory = await pathApi.join(stagingRoot, createNativeExportStagingDirectoryName());

    await invokeApi('plugin:fs|mkdir', {
        path: stagingDirectory,
        options: {
            recursive: true,
        },
    });

    const cleanup = async () => {
        await invokeApi('plugin:fs|remove', {
            path: stagingDirectory,
            options: {
                recursive: true,
            },
        });
    };

    try {
        const normalizedFileName = sanitizeDownloadFileName(fileName, fallbackName);
        const stagedFilePath = await pathApi.join(
            stagingDirectory,
            normalizedFileName,
        );

        await writeBlobToPath(invokeApi, stagedFilePath, blob);

        return {
            filePath: stagedFilePath,
            fileName: normalizedFileName,
            cleanup,
        };
    } catch (error) {
        try {
            await cleanup();
        } catch (cleanupError) {
            console.warn(`Failed to cleanup ${platformName || 'mobile'} export staging directory after staging error:`, cleanupError);
        }

        throw error;
    }
}

function getAndroidPublicDownloadBridge() {
    if (typeof window === 'undefined') {
        return null;
    }

    return window[ANDROID_PUBLIC_DOWNLOAD_BRIDGE_NAME] || null;
}

function resolveBlobMimeType(blob) {
    const mimeType = String(blob?.type || '')
        .split(';', 1)[0]
        .trim()
        .toLowerCase();

    return MIME_TYPE_PATTERN.test(mimeType) ? mimeType : DEFAULT_MIME_TYPE;
}

function parseAndroidPublicDownloadResult(rawResult) {
    const rawText = String(rawResult || '').trim();
    if (!rawText) {
        throw new Error('Android public download bridge returned an empty result');
    }

    const result = JSON.parse(rawText);
    if (!result || typeof result !== 'object') {
        throw new Error('Android public download bridge returned an invalid result');
    }

    return result;
}

function ensureAndroidPublicDownloadPickerReceiver() {
    if (typeof window === 'undefined') {
        return;
    }

    if (window[ANDROID_PUBLIC_DOWNLOAD_PICKER_RECEIVER]?.onNativeResult) {
        return;
    }

    window[ANDROID_PUBLIC_DOWNLOAD_PICKER_RECEIVER] = {
        onNativeResult(payload) {
            const pending = androidPublicDownloadPickerPending;
            androidPublicDownloadPickerPending = null;
            if (!pending) {
                return;
            }

            const error = String(payload?.error || '').trim();
            if (error) {
                pending.reject(new Error(error));
                return;
            }

            const contentUri = String(payload?.content_uri || '').trim();
            if (!contentUri) {
                pending.reject(new Error('Android export picker did not return a content URI'));
                return;
            }

            pending.resolve(contentUri);
        },
    };
}

function pickAndroidPublicDownloadDestination(bridge, fileName, mimeType) {
    if (typeof bridge.requestCreateDocumentPicker !== 'function') {
        throw new Error('Android public download picker bridge is unavailable');
    }

    ensureAndroidPublicDownloadPickerReceiver();
    if (androidPublicDownloadPickerPending) {
        throw new Error('Android export picker is already active');
    }

    return new Promise((resolve, reject) => {
        androidPublicDownloadPickerPending = { resolve, reject };

        try {
            bridge.requestCreateDocumentPicker(fileName, mimeType);
        } catch (error) {
            androidPublicDownloadPickerPending = null;
            reject(error);
        }
    });
}

function saveStagedFileToAndroidPublicDownloads(bridge, staged, mimeType) {
    if (typeof bridge.saveFileToDownloads !== 'function') {
        throw new Error('Android public download bridge is unavailable');
    }

    const result = parseAndroidPublicDownloadResult(
        bridge.saveFileToDownloads(staged.filePath, staged.fileName, mimeType),
    );
    const savedPath = String(result.saved_path || '').trim();
    if (!savedPath) {
        throw new Error('Android public download bridge did not return a saved path');
    }

    return {
        mode: 'mobile-native',
        savedPath,
        uri: String(result.uri || '').trim(),
        displayName: String(result.display_name || staged.fileName).trim(),
    };
}

async function saveStagedFileWithAndroidDocumentPicker(bridge, staged, mimeType) {
    if (typeof bridge.copyFileToContentUri !== 'function') {
        throw new Error('Android public download content bridge is unavailable');
    }

    const contentUri = await pickAndroidPublicDownloadDestination(bridge, staged.fileName, mimeType);
    const savedTarget = String(bridge.copyFileToContentUri(staged.filePath, contentUri)).trim();

    return {
        mode: 'android-document-picker',
        savedPath: '',
        uri: savedTarget || contentUri,
        displayName: staged.fileName,
    };
}

async function saveBlobWithAndroidPublicDownloadRuntime(blob, fileName, { fallbackName } = {}) {
    const bridge = getAndroidPublicDownloadBridge();
    if (!bridge) {
        throw new Error('Android public download bridge is unavailable');
    }
    if (typeof bridge.supportsDirectPublicDownloads !== 'function') {
        throw new Error('Android public download capability bridge is unavailable');
    }

    const pathApi = getPathApi();
    const stagingBaseRoot = await resolveAndroidExportStagingDirectory(pathApi);
    const staged = await stageBlobForNativeFileBridge(blob, fileName, {
        fallbackName,
        platformName: 'Android',
        stagingBaseRoot,
        stagingRootName: ANDROID_EXPORT_STAGING_ROOT_NAME,
    });
    const mimeType = resolveBlobMimeType(blob);

    try {
        if (bridge.supportsDirectPublicDownloads()) {
            return saveStagedFileToAndroidPublicDownloads(bridge, staged, mimeType);
        }

        return await saveStagedFileWithAndroidDocumentPicker(bridge, staged, mimeType);
    } finally {
        try {
            await staged.cleanup();
        } catch (error) {
            console.warn('Failed to cleanup Android export staging directory:', error);
        }
    }
}

async function shareBlobWithIosRuntime(blob, fileName, { fallbackName } = {}) {
    const invokeApi = getInvokeApi();
    const staged = await stageBlobForNativeFileBridge(blob, fileName, {
        fallbackName,
        platformName: 'iOS',
    });

    let shareResult;
    let shareError = null;
    let cleanupError = null;

    try {
        shareResult = await invokeApi('ios_share_file', {
            filePath: staged.filePath,
        });
    } catch (error) {
        shareError = error;
    }

    try {
        await staged.cleanup();
    } catch (error) {
        cleanupError = error;
        console.warn('Failed to cleanup iOS export staging directory:', error);
    }

    if (shareError) {
        throw shareError;
    }

    return {
        mode: 'ios-native-share',
        savedPath: '',
        completed: Boolean(shareResult?.completed),
        activity: typeof shareResult?.activity === 'string' && shareResult.activity.trim()
            ? shareResult.activity
            : null,
        cleanupError: cleanupError ? String(cleanupError?.message || cleanupError) : null,
    };
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

    if (isTauriRuntime()) {
        switch (hostPlatform()) {
            case 'ios':
                return shareBlobWithIosRuntime(payload, fileName, { fallbackName });
            case 'android':
                return saveBlobWithAndroidPublicDownloadRuntime(payload, fileName, { fallbackName });
            case 'windows':
            case 'macos':
            case 'linux':
                break;
            case 'ohos':
                throw new Error('File export is not implemented for OpenHarmony yet');
            default:
                throw new Error(`Unsupported file export platform: ${hostPlatform()}`);
        }
    }

    triggerBrowserDownload(payload, fileName, { fallbackName });
    return { mode: 'browser', savedPath: '' };
}
