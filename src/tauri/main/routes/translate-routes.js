import { extractErrorText, resolveHostErrorResponse } from '../kernel/host-error-response.js';

function normalizeProvider(wildcard) {
    const value = String(wildcard || '').trim();
    if (!value) {
        return '';
    }

    return value.replace(/^\/+/, '').split('/')[0] || '';
}

function errorTextResponse(message, textResponse) {
    const resolved = resolveHostErrorResponse(message);
    return textResponse(resolved.body, resolved.status, resolved.body);
}

export function registerTranslateRoutes(router, context, { textResponse }) {
    router.post('/api/translate/*', async ({ body, wildcard }) => {
        const provider = normalizeProvider(wildcard);
        if (!provider) {
            return errorTextResponse('Not found: Missing translate provider', textResponse);
        }

        try {
            const translated = await context.safeInvoke('translate_text', {
                provider,
                body,
            });
            return textResponse(translated ?? '');
        } catch (error) {
            const message = extractErrorText(error);
            return errorTextResponse(message, textResponse);
        }
    });
}
