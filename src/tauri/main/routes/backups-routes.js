import { sanitizeAttachmentFileName } from '../binary-utils.js';

function isNotFoundError(error) {
    const message = String(error?.message || error || '').toLowerCase();
    return message.includes('not found')
        || message.includes('no such file')
        || message.includes('enoent')
        || message.includes('os error 2');
}

function isBadRequestError(error) {
    const message = String(error?.message || error || '').toLowerCase();
    return message.includes('bad request')
        || message.includes('validation error')
        || message.includes('invalid');
}

async function discardChatBackupMaterialization(context, path) {
    await context.safeInvoke('discard_chat_backup_materialization', { path });
}

async function createChatBackupStream(context, path) {
    if (typeof context.createReadableFileStream !== 'function') {
        throw new Error('Readable file stream service is unavailable');
    }

    const sourceStream = await context.createReadableFileStream(path);
    const reader = sourceStream.getReader();
    let discarded = false;

    async function discardOnce() {
        if (discarded) {
            return;
        }

        discarded = true;
        try {
            await discardChatBackupMaterialization(context, path);
        } catch (error) {
            console.warn('Failed to discard chat backup materialization after streaming:', error);
        }
    }

    return new ReadableStream({
        async pull(controller) {
            try {
                const { done, value } = await reader.read();
                if (done) {
                    await discardOnce();
                    controller.close();
                    return;
                }
                controller.enqueue(value);
            } catch (error) {
                await discardOnce();
                throw error;
            }
        },
        async cancel(reason) {
            try {
                await reader.cancel(reason);
            } finally {
                await discardOnce();
            }
        },
    });
}

export function registerBackupsRoutes(router, context, { jsonResponse, textResponse }) {
    router.post('/api/backups/chat/get', async () => {
        try {
            const backups = await context.safeInvoke('list_chat_backups');
            const mapped = Array.isArray(backups)
                ? backups.map((entry) => ({
                    file_name: context.ensureJsonl(entry.file_name || ''),
                    file_size: context.formatFileSize(entry.file_size),
                    chat_items: Number(entry.message_count || 0),
                    message_count: Number(entry.message_count || 0),
                    preview_message: String(entry.preview || ''),
                    last_mes: Number(entry.date || 0),
                }))
                : [];

            return jsonResponse(mapped);
        } catch (error) {
            console.error('Failed to list chat backups:', error);
            return textResponse('Internal Server Error', 500);
        }
    });

    router.post('/api/backups/chat/delete', async ({ body }) => {
        const name = String(body?.name || '').trim();
        if (!name) {
            return textResponse('Bad Request', 400);
        }

        try {
            await context.safeInvoke('delete_chat_backup', { name });
            return textResponse('OK');
        } catch (error) {
            if (isNotFoundError(error)) {
                return textResponse('Not Found', 404);
            }

            if (isBadRequestError(error)) {
                return textResponse('Bad Request', 400);
            }

            console.error('Failed to delete chat backup:', error);
            return textResponse('Internal Server Error', 500);
        }
    });

    router.post('/api/backups/chat/download', async ({ body }) => {
        const name = String(body?.name || '').trim();
        if (!name) {
            return textResponse('Bad Request', 400);
        }

        let materializedPath = '';
        try {
            materializedPath = String(await context.safeInvoke('materialize_chat_backup', { name }) || '').trim();
            if (!materializedPath) {
                throw new Error('Materialized chat backup path is missing');
            }

            const stream = await createChatBackupStream(context, materializedPath);
            const fileName = sanitizeAttachmentFileName(name, 'chat_backup.jsonl');
            materializedPath = '';

            return new Response(stream, {
                status: 200,
                headers: {
                    'Content-Type': 'application/octet-stream',
                    'Content-Disposition': `attachment; filename="${encodeURI(fileName)}"`,
                },
            });
        } catch (error) {
            if (materializedPath) {
                try {
                    await discardChatBackupMaterialization(context, materializedPath);
                } catch (cleanupError) {
                    console.error('Failed to discard chat backup materialization:', cleanupError);
                }
            }

            if (isNotFoundError(error)) {
                return textResponse('Not Found', 404);
            }

            if (isBadRequestError(error)) {
                return textResponse('Bad Request', 400);
            }

            console.error('Failed to download chat backup:', error);
            return textResponse('Internal Server Error', 500);
        }
    });
}
