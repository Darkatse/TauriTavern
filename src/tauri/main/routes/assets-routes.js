import { normalizeBinaryPayload } from '../binary-utils.js';

export function registerAssetsRoutes(router, context, { jsonResponse, textResponse }) {
    router.post('/api/assets/get', async () => {
        const assets = await context.safeInvoke('get_assets_library');
        return jsonResponse(assets || {});
    });

    router.post('/api/assets/download', async ({ body }) => {
        const category = String(body?.category || '');
        const result = await context.safeInvoke('download_asset', {
            url: String(body?.url || ''),
            category,
            filename: String(body?.filename || ''),
        });

        if (category === 'character') {
            const bytes = normalizeBinaryPayload(result?.data);
            const mimeType = String(result?.mimeType || 'application/octet-stream');
            return new Response(bytes, {
                status: 200,
                headers: {
                    'Content-Type': mimeType,
                },
            });
        }

        return textResponse('OK');
    });

    router.post('/api/assets/delete', async ({ body }) => {
        await context.safeInvoke('delete_asset', {
            category: String(body?.category || ''),
            filename: String(body?.filename || ''),
        });
        return textResponse('OK');
    });

    router.post('/api/assets/character', async ({ url }) => {
        const assets = await context.safeInvoke('get_character_assets', {
            name: String(url?.searchParams?.get('name') || ''),
            category: String(url?.searchParams?.get('category') || ''),
        });
        return jsonResponse(assets || []);
    });
}
