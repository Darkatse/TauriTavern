// @ts-check

// Amortize IPC roundtrips while bounding each response and decode burst.
const FS_READ_MAX_CHUNK_BYTES = 4 * 1024 * 1024;

/** @param {any} data */
function normalizeReadResponse(data) {
    if (data instanceof Uint8Array) {
        return data;
    }

    if (data instanceof ArrayBuffer) {
        return new Uint8Array(data);
    }

    throw new Error('Unexpected resource read response');
}

/**
 * @param {{ invoke: Function }} deps
 */
export function createReadableFileStreamService({ invoke }) {
    if (typeof invoke !== 'function') {
        throw new Error('Tauri invoke API is unavailable');
    }

    /**
     * @param {Promise<number>} ridPromise
     * @param {(rid: number) => Promise<Uint8Array>} readChunk
     */
    function createReadableResourceStream(ridPromise, readChunk) {
        let closed = false;

        async function closeOnce() {
            if (closed) {
                return;
            }

            closed = true;
            const rid = await ridPromise;
            await invoke('plugin:resources|close', { rid });
        }

        return new ReadableStream({
            async pull(controller) {
                const rid = await ridPromise;
                if (closed) {
                    return;
                }

                try {
                    const bytes = await readChunk(rid);
                    if (bytes.byteLength === 0) {
                        await closeOnce();
                        controller.close();
                        return;
                    }

                    controller.enqueue(bytes);
                } catch (error) {
                    try {
                        await closeOnce();
                    } catch (closeError) {
                        throw new AggregateError([error, closeError], 'Failed to read and close resource');
                    }
                    throw error;
                }
            },
            async cancel() {
                await closeOnce();
            },
        });
    }

    /** @param {string} filePath */
    function createReadableFileStream(filePath) {
        /** @type {number | undefined} */
        let remaining;
        return createReadableResourceStream(
            invoke('plugin:fs|open', {
                path: filePath,
                options: { read: true },
            }),
            async (rid) => {
                // Keep stat inside the resource owner's error handling: open may succeed even if stat fails.
                if (remaining === undefined) {
                    /** @type {{ size: number }} */
                    const { size } = await invoke('plugin:fs|fstat', { rid });
                    if (!Number.isSafeInteger(size) || size < 0) {
                        throw new Error(`Invalid file size for ${filePath}: ${size}`);
                    }
                    remaining = size;
                }
                if (remaining === 0) {
                    return new Uint8Array(0);
                }

                // plugin-fs returns the requested length even on a short read, plus its 8-byte trailer.
                const len = Math.min(FS_READ_MAX_CHUNK_BYTES, remaining);
                const data = await invoke('plugin:fs|read', {
                    rid,
                    len,
                });
                const bytes = normalizeReadResponse(data);
                const bytesRead = Number(new DataView(bytes.buffer, bytes.byteOffset + bytes.byteLength - 8, 8).getBigUint64(0));
                if (bytesRead > len || bytesRead > bytes.byteLength - 8) {
                    throw new Error(`Invalid fs read length for ${filePath}: ${bytesRead}`);
                }
                if (bytesRead === 0) {
                    throw new Error(`File shorter than its declared size: ${filePath} (${remaining} bytes remaining)`);
                }
                remaining -= bytesRead;
                return bytes.subarray(0, bytesRead);
            },
        );
    }

    /** @param {string} name */
    async function createChatBackupDownloadStream(name) {
        const rid = await invoke('open_chat_backup_download', { name });
        return createReadableResourceStream(
            Promise.resolve(rid),
            async (rid) => normalizeReadResponse(
                await invoke('read_chat_backup_download', { rid }),
            ),
        );
    }

    return {
        createChatBackupDownloadStream,
        createReadableFileStream,
    };
}
