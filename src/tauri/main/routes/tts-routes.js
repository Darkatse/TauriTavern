import { decodeBase64ToBytes } from '../binary-utils.js';
import { textResponse } from '../http-utils.js';
import { extractErrorText, resolveHostErrorResponse } from '../kernel/host-error-response.js';

function normalizeRouteResponse(value) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        return {
            status: 500,
            contentType: 'text/plain; charset=utf-8',
            bodyBase64: '',
            statusText: 'Invalid response',
        };
    }

    const status = Number(value.status);
    const safeStatus = Number.isFinite(status) && status >= 100 && status <= 599 ? status : 500;
    const contentType = String(value.contentType || value.content_type || 'application/octet-stream').trim()
        || 'application/octet-stream';
    const bodyBase64 = String(value.bodyBase64 || value.body_base64 || '');
    const statusText = String(value.statusText || value.status_text || '').trim();

    return {
        status: safeStatus,
        contentType,
        bodyBase64,
        statusText,
    };
}

function createRouteResponse(payload) {
    const response = normalizeRouteResponse(payload);
    const init = {
        status: response.status,
        headers: {
            'Content-Type': response.contentType,
        },
    };

    if (response.statusText) {
        init.statusText = response.statusText;
    }

    return new Response(decodeBase64ToBytes(response.bodyBase64), init);
}

async function handleTtsRoute(context, path, body) {
    try {
        const payload = await context.safeInvoke('tts_handle', {
            path,
            body: body || {},
        });

        return createRouteResponse(payload);
    } catch (error) {
        const resolved = resolveHostErrorResponse(extractErrorText(error));
        return textResponse(resolved.body, resolved.status, resolved.body);
    }
}

export function registerTtsRoutes(router, context) {
    router.post('/api/tts/grok/voices', async ({ body }) => {
        return handleTtsRoute(context, 'grok/voices', body);
    });

    router.post('/api/tts/grok/generate', async ({ body }) => {
        return handleTtsRoute(context, 'grok/generate', body);
    });

    router.post('/api/tts/mimo/generate', async ({ body }) => {
        return handleTtsRoute(context, 'mimo/generate', body);
    });
}
