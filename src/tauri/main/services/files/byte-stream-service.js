// @ts-check
import { readByteResponse } from '../../kernel/byte-protocol.js';

/**
 * @param {{ invoke: Function }} deps
 */
export function createByteStreamService({ invoke }) {
    if (typeof invoke !== 'function') {
        throw new Error('Tauri invoke API is unavailable');
    }

    /** @param {number} rid A host byte resource, drained with `read_bytes`. */
    function createByteStream(rid) {
        let closed = false;

        async function closeOnce() {
            if (closed) {
                return;
            }

            closed = true;
            await invoke('plugin:resources|close', { rid });
        }

        return new ReadableStream({
            async pull(controller) {
                if (closed) {
                    return;
                }

                try {
                    const bytes = readByteResponse(await invoke('read_bytes', { rid }));
                    if (closed) return;
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

    /** @param {string} name */
    async function createChatBackupDownloadStream(name) {
        const rid = await invoke('open_chat_backup_download', { name });
        return createByteStream(rid);
    }

    return {
        createChatBackupDownloadStream,
        createByteStream,
    };
}
