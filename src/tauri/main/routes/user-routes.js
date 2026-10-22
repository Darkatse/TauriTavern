import { extractErrorText, resolveHostErrorResponse } from '../kernel/host-error-response.js';

export function registerUserRoutes(router, context, { jsonResponse }) {
    router.post('/api/users/backup', async ({ body }) => {
        const handle = String(body?.handle || '').trim();
        if (!handle) {
            return jsonResponse({ error: 'Bad request: User handle is required for backup' }, 400);
        }
        try {
            const secretSettings = await context.safeInvoke('read_secret_settings');
            const includeSecrets = secretSettings?.allowKeysExposure === true;
            const result = await context.safeInvoke('export_user_backup_archive', {
                handle,
                include_secrets: includeSecrets,
            });
            return jsonResponse({ ok: true, delivered: result.delivered, includes_secrets: includeSecrets });
        } catch (error) {
            const resolved = resolveHostErrorResponse(extractErrorText(error));
            return jsonResponse({ error: resolved.body }, resolved.status);
        }
    });
}
