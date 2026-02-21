import { normalizeBinaryPayload, sanitizeAttachmentFileName } from '../binary-utils.js';

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

        try {
            const payload = await context.safeInvoke('get_chat_backup_raw', { name });
            const bytes = normalizeBinaryPayload(payload);
            const fileName = sanitizeAttachmentFileName(name, 'chat_backup.jsonl');

            return new Response(bytes, {
                status: 200,
                headers: {
                    'Content-Type': 'application/octet-stream',
                    'Content-Disposition': `attachment; filename="${encodeURI(fileName)}"`,
                },
            });
        } catch (error) {
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
