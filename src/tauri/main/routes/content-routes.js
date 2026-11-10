import { stagedCardBytes } from './character-route-utils.js';

export function registerContentRoutes(router, context, { jsonResponse }) {
    router.post('/api/content/importURL', async ({ body }) => {
        const card = await stagedCardBytes(context, String(body?.url || ''));
        return new Response(card.bytes, {
            status: 200,
            headers: {
                'Content-Type': card.type,
                'Content-Disposition': `attachment; filename="${encodeURI(card.name)}"`,
                'X-Custom-Content-Type': 'character',
            },
        });
    });

    router.post('/api/content/importUUID', async () => {
        return jsonResponse(
            { error: 'UUID import is not supported in this Tauri build. Use a direct URL.' },
            501,
        );
    });
}
