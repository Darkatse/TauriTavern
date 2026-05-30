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
function normalizeFsReadResponse(data) {
    if (data instanceof Uint8Array) {
        return data;
    }

    if (data instanceof ArrayBuffer) {
        return new Uint8Array(data);
    }

    throw new Error('Unexpected fs read response');
}

/**
 * @param {{ invoke: Function }} deps
 */
export function createReadableFileStreamService({ invoke }) {
    if (typeof invoke !== 'function') {
        throw new Error('Tauri invoke API is unavailable');
    }

    /** @param {string} filePath */
    function createReadableFileStream(filePath) {
        const ridPromise = invoke('plugin:fs|open', {
            path: filePath,
            options: { read: true },
        });
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
                    const data = await invoke('plugin:fs|read', {
                        rid,
                        len: FS_READ_CHUNK_BYTES,
                    });
                    const bytes = normalizeFsReadResponse(data);
                    const trailer = bytes.subarray(bytes.byteLength - 8);
                    const bytesRead = readBigEndianUint64(trailer);

                    if (bytesRead === 0) {
                        await closeOnce();
                        controller.close();
                        return;
                    }

                    controller.enqueue(bytes.subarray(0, bytesRead));
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

    return {
        createReadableFileStream,
    };
}
