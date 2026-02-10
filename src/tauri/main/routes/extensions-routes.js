export function registerExtensionRoutes(router, context, { jsonResponse }) {
    router.all('/api/extensions/discover', async () => {
        const extensions = await context.safeInvoke('get_extensions');
        const mapped = Array.isArray(extensions)
            ? extensions.map((extension) => ({
                name: extension.name,
                type: String(extension.extension_type || 'local').toLowerCase(),
            }))
            : [];

        return jsonResponse(mapped);
    });

    router.post('/api/extensions/install', async ({ body }) => {
        const result = await context.safeInvoke('install_extension', {
            url: body?.url || '',
            global: Boolean(body?.global),
        });

        return jsonResponse({
            display_name: result?.display_name || body?.url || 'Extension',
            author: result?.author || 'Unknown',
            version: result?.version || '0.0.0',
            extensionPath: result?.extension_path || '',
        });
    });

    router.post('/api/extensions/update', async ({ body }) => {
        const result = await context.safeInvoke('update_extension', {
            extension_name: body?.extensionName || '',
            global: Boolean(body?.global),
        });

        return jsonResponse({
            isUpToDate: Boolean(result?.is_up_to_date),
            shortCommitHash: result?.short_commit_hash || 'unknown',
        });
    });

    router.post('/api/extensions/delete', async ({ body }) => {
        await context.safeInvoke('delete_extension', {
            extension_name: body?.extensionName || '',
            global: Boolean(body?.global),
        });

        return jsonResponse({ ok: true });
    });

    router.post('/api/extensions/version', async ({ body }) => {
        const result = await context.safeInvoke('get_extension_version', {
            extension_name: body?.extensionName || '',
            global: Boolean(body?.global),
        });

        return jsonResponse({
            currentBranchName: result?.current_branch_name || 'main',
            currentCommitHash: result?.current_commit_hash || 'unknown',
            isUpToDate: Boolean(result?.is_up_to_date),
            remoteUrl: result?.remote_url || '',
        });
    });

    router.post('/api/extensions/move', async ({ body }) => {
        await context.safeInvoke('move_extension', {
            extension_name: body?.extensionName || '',
            source: body?.source || 'local',
            destination: body?.destination || 'global',
        });

        return jsonResponse({ ok: true });
    });

    router.post('/api/extensions/branches', async () => jsonResponse([]));
    router.post('/api/extensions/switch', async () => jsonResponse({ error: 'Branch switching is not supported in Tauri backend' }, 400));
}
