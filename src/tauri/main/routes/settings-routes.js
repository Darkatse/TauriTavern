import { commitBytes } from '../services/files/byte-commit.js';
import { textFragmentsToByteChunks } from '../kernel/utf8.js';

const PROVIDER_METADATA_SECRET_KEYS = new Set([
    'api_key_openrouter',
    'api_key_nanogpt',
    'api_key_siliconflow',
    'api_key_workers_ai',
]);

const SECRET_BACKED_PROVIDER_METADATA_COMMANDS = [
    'get_openrouter_credits',
    'get_nanogpt_credits',
    'get_siliconflow_embedding_models',
    'get_workers_ai_embedding_models',
    'get_workers_ai_multimodal_models',
];

function invalidateSecretBackedProviderMetadata(context, key) {
    if (!PROVIDER_METADATA_SECRET_KEYS.has(String(key || ''))) {
        return;
    }

    for (const command of SECRET_BACKED_PROVIDER_METADATA_COMMANDS) {
        context.invalidateInvokeAll(command);
    }
}

export function registerSettingsRoutes(router, context, { jsonResponse }) {
    router.post('/api/settings/get', async () => {
        const bytes = await context.safeInvoke('get_sillytavern_settings');
        return new Response(bytes, { headers: { 'Content-Type': 'application/json' } });
    });

    router.post('/api/settings/save', async ({ body, input, init }) => {
        const headers = new Headers(init?.headers ?? input?.headers);
        const revision = headers.get('X-TauriTavern-Settings-Revision');
        let expectedRevision = null;
        if (revision !== null) {
            try {
                expectedRevision = JSON.parse(revision);
                if (expectedRevision === null) throw new Error('Empty revision');
            } catch {
                return jsonResponse({ error: 'Invalid settings revision header' }, 400);
            }
        }
        const text = typeof body === 'string' ? body : JSON.stringify(body);
        if (typeof text !== 'string') {
            return jsonResponse({ error: 'Settings replacement requires JSON' }, 400);
        }
        const result = await commitBytes({
            begin: () => context.invokeTransport('begin_settings_commit', { expectedRevision }),
            frames: maxBytes => textFragmentsToByteChunks([text], maxBytes),
            append: (data, options) => context.invokeTransport('append_settings_commit_chunk', data, options),
            finish: (sessionId, expectedSize) => context.invokeTransport('finish_settings_commit', {
                sessionId,
                expectedSize,
            }),
            abort: sessionId => context.invokeTransport('abort_settings_commit', { sessionId }),
        });
        return jsonResponse(result);
    }, { body: 'text' });

    router.post('/api/settings/make-snapshot', async () => {
        await context.safeInvoke('create_settings_snapshot');
        return jsonResponse({ result: 'ok' });
    });

    router.post('/api/settings/get-snapshots', async () => {
        const snapshots = await context.safeInvoke('get_settings_snapshots');
        return jsonResponse(snapshots);
    });

    router.post('/api/settings/load-snapshot', async ({ body }) => {
        const name = body?.name || '';
        const snapshot = await context.safeInvoke('load_settings_snapshot', { name });
        return jsonResponse(snapshot?.data || snapshot || {});
    });

    router.post('/api/settings/restore-snapshot', async ({ body }) => {
        const name = body?.name || '';
        await context.safeInvoke('restore_settings_snapshot', { name });
        return jsonResponse({ result: 'ok' });
    });

    router.post('/api/secrets/read', async () => {
        try {
            const state = await context.safeInvoke('read_secret_state');
            return jsonResponse(state || {});
        } catch (error) {
            console.warn('Failed to read secret state:', error);
            return jsonResponse({ error: 'secret_state_unavailable' }, 503);
        }
    });

    router.post('/api/secrets/settings', async () => {
        const settings = await context.safeInvoke('read_secret_settings');
        return jsonResponse(settings);
    });

    router.post('/api/secrets/find', async ({ body }) => {
        const key = body?.key || '';
        const id = body?.id || null;
        const result = await context.safeInvoke('find_secret', { dto: { key, id } });
        return jsonResponse(result || { value: '' });
    });

    router.post('/api/secrets/write', async ({ body }) => {
        const key = body?.key || '';
        const value = body?.value || '';
        const label = body?.label ?? null;
        const id = await context.safeInvoke('write_secret', { dto: { key, value, label } });
        invalidateSecretBackedProviderMetadata(context, key);
        return jsonResponse({ id });
    });

    router.post('/api/secrets/delete', async ({ body }) => {
        const key = body?.key || '';
        const id = body?.id || null;
        await context.safeInvoke('delete_secret', { dto: { key, id } });
        invalidateSecretBackedProviderMetadata(context, key);
        return jsonResponse({ ok: true });
    });

    router.post('/api/secrets/rotate', async ({ body }) => {
        const key = body?.key || '';
        const id = body?.id || '';
        await context.safeInvoke('rotate_secret', { dto: { key, id } });
        invalidateSecretBackedProviderMetadata(context, key);
        return jsonResponse({ ok: true });
    });

    router.post('/api/secrets/rename', async ({ body }) => {
        const key = body?.key || '';
        const id = body?.id || '';
        const label = body?.label || '';
        await context.safeInvoke('rename_secret', { dto: { key, id, label } });
        return jsonResponse({ ok: true });
    });

    router.post('/api/secrets/view', async () => {
        try {
            const secrets = await context.safeInvoke('view_secrets');
            return jsonResponse(secrets || {});
        } catch {
            return jsonResponse({ error: 'Forbidden' }, 403);
        }
    });

    router.post('/api/presets/save', async ({ body }) => {
        const result = await context.safeInvoke('save_preset', {
            dto: {
                name: body?.name || '',
                apiId: body?.apiId || '',
                preset: body?.preset || {},
            },
        });

        return jsonResponse(result || { name: body?.name || '' });
    });

    router.post('/api/presets/delete', async ({ body }) => {
        await context.safeInvoke('delete_preset', {
            dto: {
                name: body?.name || '',
                apiId: body?.apiId || '',
            },
        });

        return jsonResponse({ ok: true });
    });

    router.post('/api/presets/restore', async ({ body }) => {
        const result = await context.safeInvoke('restore_preset', {
            dto: {
                name: body?.name || '',
                apiId: body?.apiId || '',
            },
        });

        return jsonResponse(result || { isDefault: false, preset: {} });
    });
}
