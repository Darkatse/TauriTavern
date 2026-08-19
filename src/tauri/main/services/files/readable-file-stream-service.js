// @ts-check

const FS_READ_CHUNK_BYTES = 512 * 1024;

/** @param {Uint8Array} bytes */
function readBigEndianUint64(bytes) {
    let value = 0;
    for (let i = 0; i < bytes.length; i += 1) {
        const byte = bytes[i];
        if (byte === undefined) {
            throw new Error('Unexpected fs read trailer byte');
        }
        value *= 0x100;
        value += byte;
    }
    return value;
}

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

                try {
                    const bytes = await readChunk(rid);
                    if (bytes.byteLength === 0) {
                        await closeOnce();
                        controller.close();
                        return;
                    }

                    controller.enqueue(bytes);
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

    /** @param {string} filePath */
    function createReadableFileStream(filePath) {
        return createReadableResourceStream(
            invoke('plugin:fs|open', {
                path: filePath,
                options: { read: true },
            }),
            async (rid) => {
                const data = await invoke('plugin:fs|read', {
                    rid,
                    len: FS_READ_CHUNK_BYTES,
                });
                const bytes = normalizeReadResponse(data);
                const bytesRead = readBigEndianUint64(bytes.subarray(bytes.byteLength - 8));
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
