import { getSillyTavernLocale } from '../adapters/st/sillytavern-i18n.js';
import { extractErrorText, resolveHostErrorResponse } from '../kernel/host-error-response.js';

function errorTextResponse(message, textResponse) {
    const resolved = resolveHostErrorResponse(message);
    return textResponse(resolved.body, resolved.status, resolved.body);
}

export function registerSearchRoutes(router, context, { jsonResponse, textResponse }) {
    router.post('/api/search/searxng', async ({ body }) => {
        try {
            const html = await context.safeInvoke('search_searxng', {
                dto: body,
                locale: getSillyTavernLocale(),
            });
            return new Response(String(html ?? ''), {
                headers: { 'Content-Type': 'text/html; charset=utf-8' },
            });
        } catch (error) {
            return errorTextResponse(extractErrorText(error), textResponse);
        }
    });

    router.all('/api/search/*', ({ path }) => jsonResponse({
        error: `Unsupported endpoint: ${path}`,
    }, 404));
}
