import { getSillyTavernLocale } from '../adapters/st/sillytavern-i18n.js';
import { commandErrorResponse } from '../../../scripts/util/command-error-utils.js';

function normalizeProvider(wildcard) {
    const value = String(wildcard || '').trim();
    if (!value) {
        return '';
    }

    return value.replace(/^\/+/, '').split('/')[0] || '';
}

function errorTextResponse(error, textResponse) {
    const resolved = commandErrorResponse(error);
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
                locale: getSillyTavernLocale(),
            });
            return textResponse(translated ?? '');
        } catch (error) {
            return errorTextResponse(error, textResponse);
        }
    });
}
