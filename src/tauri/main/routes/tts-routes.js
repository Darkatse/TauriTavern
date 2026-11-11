import { decodeBase64ToBytes } from '../binary-utils.js';
import { safeResponseStatusText, textResponse } from '../http-utils.js';
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
    const statusText = safeResponseStatusText(value.statusText || value.status_text);

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
    const routes = [
        '/api/azure/list',
        '/api/azure/generate',
        '/api/google/list-voices',
        '/api/google/generate-voice',
        '/api/google/list-native-voices',
        '/api/google/generate-native-tts',
        '/api/novelai/generate-voice',
        '/api/openai/generate-voice',
        '/api/openai/custom/generate-voice',
        '/api/openai/electronhub/models',
        '/api/openai/electronhub/generate-voice',
        '/api/openai/chutes/generate-voice',
        '/api/speech/elevenlabs/voices',
        '/api/speech/elevenlabs/voice-settings',
        '/api/speech/elevenlabs/synthesize',
        '/api/speech/elevenlabs/history',
        '/api/speech/elevenlabs/history-audio',
        '/api/speech/elevenlabs/voices/add',
        '/api/speech/pollinations/voices',
        '/api/speech/pollinations/generate',
        '/api/volcengine/generate-voice',
        '/api/tts/grok/voices',
        '/api/tts/grok/generate',
        '/api/tts/mimo/generate',
        '/api/minimax/generate-voice',
    ];

    for (const route of routes) {
        const path = route.replace(/^\/api\/(?:tts\/)?/, '');
        router.post(route, async ({ body }) => handleTtsRoute(context, path, body));
    }
}
