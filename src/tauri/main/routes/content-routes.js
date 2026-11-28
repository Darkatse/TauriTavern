import { stageUrlContent, takeUrlContent } from '../services/files/url-content.js';

export function registerContentRoutes(router, context) {
    async function importContent({ body }) {
        const content = await stageUrlContent(context.safeInvoke, String(body?.url || ''));
        const { bytes, type } = await takeUrlContent(context, content);
        return new Response(bytes, {
            status: 200,
            headers: {
                'Content-Type': type,
                'Content-Disposition': `attachment; filename="${encodeURI(content.name)}"`,
                'X-Custom-Content-Type': content.kind,
            },
        });
    }

    router.post('/api/content/importURL', importContent);
    router.post('/api/content/importUUID', importContent);
}
