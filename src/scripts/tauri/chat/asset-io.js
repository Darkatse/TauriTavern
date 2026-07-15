import { convertFileSrc, invoke } from '../../../tauri-bridge.js';
import { encodeBytesToBase64 } from '../../../tauri/main/binary-utils.js';
import { isAndroidRuntime } from './platform.js';

function requireTauri() {
    if (typeof window === 'undefined' || typeof window.__TAURI__ !== 'object') {
        throw new Error('Tauri runtime is required');
    }

    return window.__TAURI__;
}

export async function writeTempFileFromBytesIterable(bytesIterable) {
    let filePath = '';

    try {
        const begin = await invoke('stage_upload_begin', {
            dto: {
                kind: 'chat-jsonl',
                preferred_extension: 'jsonl',
            },
        });
        filePath = String(begin?.file_path || '').trim();
        if (!filePath) {
            throw new Error('Host chat staging did not return a file path');
        }

        const chunkSize = Number(begin?.chunk_size);
        if (!Number.isSafeInteger(chunkSize) || chunkSize <= 0) {
            throw new Error('Host chat staging returned an invalid chunk size');
        }

        const android = isAndroidRuntime();
        let offset = 0;

        for (const inputChunk of bytesIterable) {
            if (!(inputChunk instanceof Uint8Array)) {
                throw new Error('Chat staging input must contain Uint8Array chunks');
            }

            for (let start = 0; start < inputChunk.byteLength; start += chunkSize) {
                const frame = inputChunk.subarray(start, start + chunkSize);
                const headers = {
                    'file-path': encodeURIComponent(filePath),
                    offset: String(offset),
                };
                const nextOffset = Number(await (android
                    ? invoke('stage_upload_chunk', { data: encodeBytesToBase64(frame) }, {
                        headers: {
                            ...headers,
                            'chunk-encoding': 'base64',
                        },
                    })
                    : invoke('stage_upload_chunk', frame, { headers })));
                const expectedNextOffset = offset + frame.byteLength;
                if (nextOffset !== expectedNextOffset) {
                    throw new Error(`Host chat staging returned unexpected offset ${nextOffset}`);
                }
                offset = nextOffset;
            }
        }

        const finished = await invoke('stage_upload_finish', {
            file_path: filePath,
            expected_size: offset,
        });
        const finishedPath = String(finished?.file_path || '').trim();
        if (!finishedPath) {
            throw new Error('Host chat staging did not return a finished file path');
        }
        if (Number(finished?.size) !== offset) {
            throw new Error(`Host chat staging returned unexpected size ${finished?.size}`);
        }

        return {
            filePath: finishedPath,
            cleanup: () => invoke('stage_upload_discard', {
                file_path: finishedPath,
            }),
        };
    } catch (error) {
        if (filePath) {
            try {
                await invoke('stage_upload_discard', { file_path: filePath });
            } catch {
                // Preserve the staging error; an orphaned cache file is non-fatal.
            }
        }
        throw error;
    }
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
