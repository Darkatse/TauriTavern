import { getSillyTavernLocale } from '../adapters/st/sillytavern-i18n.js';
import { stagedCardBytes } from './character-route-utils.js';

export function registerAssetsRoutes(router, context, { jsonResponse, textResponse }) {
    router.post('/api/assets/get', async () => {
        const assets = await context.safeInvoke('get_assets_library');
        return jsonResponse(assets || {});
    });

    router.post('/api/assets/download', async ({ body }) => {
        const url = String(body?.url || '');
        const category = String(body?.category || '');
        if (category === 'character') {
            const card = await stagedCardBytes(context, url);
            return new Response(card.bytes, { status: 200, headers: { 'Content-Type': card.type } });
        }

        await context.safeInvoke('download_asset', {
            url,
            category,
            filename: String(body?.filename || ''),
            locale: getSillyTavernLocale(),
        });
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
