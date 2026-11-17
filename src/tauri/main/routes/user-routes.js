import { commandErrorResponse } from '../../../scripts/util/command-error-utils.js';

export function registerUserRoutes(router, context, { jsonResponse }) {
    router.post('/api/users/backup', async ({ body }) => {
        const handle = String(body?.handle || '').trim();
        if (!handle) {
            return jsonResponse({ error: 'Bad request: User handle is required for backup' }, 400);
        }
        try {
            const result = await context.safeInvoke('export_user_backup_archive', { handle });
            return jsonResponse({ ok: true, delivered: result.delivered, includes_secrets: result.includes_secrets });
        } catch (error) {
            const resolved = commandErrorResponse(error);
            return jsonResponse({ error: resolved.body }, resolved.status);
        }
    });
}
