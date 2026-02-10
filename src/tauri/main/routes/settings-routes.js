export function registerSettingsRoutes(router, context, { jsonResponse }) {
    router.post('/api/settings/get', async () => {
        const settings = await context.safeInvoke('get_sillytavern_settings');
        return jsonResponse(settings);
    });

    router.post('/api/settings/save', async ({ body }) => {
        await context.safeInvoke('save_user_settings', { settings: body || {} });
        return jsonResponse({ result: 'ok' });
    });

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
        const state = await context.safeInvoke('read_secret_state');
        return jsonResponse(state || {});
    });

    router.post('/api/secrets/find', async ({ body }) => {
        const key = body?.key || '';
        const result = await context.safeInvoke('find_secret', { dto: { key } });
        return jsonResponse(result || { value: '' });
    });

    router.post('/api/secrets/write', async ({ body }) => {
        const key = body?.key || '';
        const value = body?.value || '';
        await context.safeInvoke('write_secret', { dto: { key, value } });
        return jsonResponse({ id: 'default' });
    });

    router.post('/api/secrets/delete', async ({ body }) => {
        const key = body?.key || '';
        await context.safeInvoke('write_secret', { dto: { key, value: '' } });
        return jsonResponse({ ok: true });
    });

    router.post('/api/secrets/rotate', async () => jsonResponse({ ok: true }));
    router.post('/api/secrets/rename', async () => jsonResponse({ ok: true }));

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
