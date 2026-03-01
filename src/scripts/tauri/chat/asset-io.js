import { convertFileSrc } from '../../../tauri-bridge.js';
import { isAndroidRuntime } from './platform.js';

function requireTauri() {
    if (typeof window === 'undefined' || typeof window.__TAURI__ !== 'object') {
        throw new Error('Tauri runtime is required');
    }

    return window.__TAURI__;
}

async function resolveTempDir(pathApi) {
    const candidates = isAndroidRuntime()
        ? [pathApi.appCacheDir, pathApi.tempDir]
        : [pathApi.tempDir, pathApi.appCacheDir];

    for (const candidate of candidates) {
        if (typeof candidate === 'function') {
            return candidate();
        }
    }

    throw new Error('No writable temp directory is available');
}

async function fsWriteFileChunk(invokeApi, filePath, bytes, { append }) {
    const writeOptions = {
        append: Boolean(append),
        create: true,
    };

    await invokeApi('plugin:fs|write_file', bytes, {
        headers: {
            path: encodeURIComponent(filePath),
            options: JSON.stringify(writeOptions),
        },
    });
}

export async function writeTempFileFromBytesIterable(bytesIterable, { prefix, extension = 'jsonl' } = {}) {
    const normalizedPrefix = String(prefix || 'temp').trim() || 'temp';
    const normalizedExtension = String(extension || '').trim().replace(/^\./, '');
    const tauri = requireTauri();
    const pathApi = tauri.path;
    const invokeApi = tauri.core?.invoke;

    if (!pathApi || typeof pathApi.join !== 'function') {
        throw new Error('Tauri path API is unavailable');
    }

    if (typeof invokeApi !== 'function') {
        throw new Error('Tauri invoke API is unavailable');
    }

    const tempDir = await resolveTempDir(pathApi);
    const suffix = normalizedExtension ? `.${normalizedExtension}` : '';
    const fileName = `${normalizedPrefix}-${Date.now()}-${Math.random().toString(16).slice(2)}${suffix}`;
    const filePath = await pathApi.join(tempDir, fileName);

    let append = false;
    for (const chunk of bytesIterable) {
        if (chunk.byteLength === 0) {
            continue;
        }

        await fsWriteFileChunk(invokeApi, filePath, chunk, { append });
        append = true;
    }

    if (!append) {
        await fsWriteFileChunk(invokeApi, filePath, new Uint8Array(0), { append: false });
    }

    return {
        filePath,
        cleanup: async () => {
            await invokeApi('plugin:fs|remove', { path: filePath });
        },
    };
}

const FS_READ_CHUNK_BYTES = 512 * 1024;

function readBigEndianUint64(bytes) {
    let value = 0;

    for (let i = 0; i < bytes.length; i += 1) {
        value *= 0x100;
        value += bytes[i];
    }

    return value;
}

function normalizeFsReadResponse(data) {
    if (data instanceof Uint8Array) {
        return data;
    }

    if (data instanceof ArrayBuffer) {
        return new Uint8Array(data);
    }

    throw new Error('Unexpected fs read response');
}

async function fsReadIntoBuffer(invokeApi, rid, buffer) {
    const data = await invokeApi('plugin:fs|read', { rid, len: buffer.byteLength });
    const bytes = normalizeFsReadResponse(data);
    const trailer = bytes.subarray(bytes.byteLength - 8);
    const bytesRead = readBigEndianUint64(trailer);
    buffer.set(bytes.subarray(0, bytes.byteLength - 8));
    return bytesRead === 0 ? null : bytesRead;
}

function createFsReadableStream(filePath) {
    const tauri = requireTauri();
    const invokeApi = tauri.core?.invoke;

    if (typeof invokeApi !== 'function') {
        throw new Error('Tauri invoke API is unavailable');
    }

    const ridPromise = invokeApi('plugin:fs|open', {
        path: filePath,
        options: { read: true },
    });
    let isClosed = false;

    const closeOnce = async () => {
        if (isClosed) {
            return;
        }

        isClosed = true;
        const rid = await ridPromise;
        await invokeApi('plugin:resources|close', { rid });
    };

    return new ReadableStream({
        async pull(controller) {
            const rid = await ridPromise;

            try {
                const buffer = new Uint8Array(FS_READ_CHUNK_BYTES);
                const bytesRead = await fsReadIntoBuffer(invokeApi, rid, buffer);

                if (bytesRead === null) {
                    await closeOnce();
                    controller.close();
                    return;
                }

                if (bytesRead > 0) {
                    controller.enqueue(buffer.subarray(0, bytesRead));
                }
            } catch (error) {
                await closeOnce();
                throw error;
            }
        },
        async cancel() {
            await closeOnce();
        },
    });
}

export async function fetchAssetStream(filePath) {
    if (isAndroidRuntime()) {
        return createFsReadableStream(filePath);
    }

    const assetUrl = convertFileSrc(filePath, 'asset');
    const response = await fetch(assetUrl, { cache: 'no-store' });
    if (!response.ok) {
        throw new Error(`Failed to fetch asset payload: ${response.status}`);
    }

    if (!response.body) {
        throw new Error('Asset response body is unavailable');
    }

    return response.body;
}
