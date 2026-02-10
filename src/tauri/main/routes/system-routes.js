export function registerSystemRoutes(router, context, { jsonResponse }) {
    router.all('/csrf-token', async () => jsonResponse({ token: 'tauri-dummy-token' }));

    router.all('/version', async () => {
        const versionInfo = await context.safeInvoke('get_client_version');
        return jsonResponse(versionInfo);
    });

    router.all('/api/ping', async () => jsonResponse({ result: 'ok' }));

    router.all('/api/modules', async () => jsonResponse([]));
}
