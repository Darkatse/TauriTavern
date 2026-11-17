// @ts-check

import { getSillyTavernLocale } from '../adapters/st/sillytavern-i18n.js';
import { commandErrorResponse } from '../../../scripts/util/command-error-utils.js';
import { copyGenerationPayload } from '../services/ai/generation-sessions.js';

/**
 * @param {string} text
 */
function buildErrorChunk(text) {
    return { choices: [{ index: 0, text }] };
}

/**
 * @param {string} text
 */
function buildErrorCompletion(text) {
    return { choices: [{ index: 0, text, finish_reason: 'stop' }] };
}

/**
 * @param {string} data
 * @returns {string}
 */
function extractChunkText(data) {
    const parsed = JSON.parse(data);
    const text = parsed?.choices?.[0]?.text ?? parsed?.content;
    return typeof text === 'string' ? text : '';
}

/**
 * Upstream's `/api/backends/text-completions/*`. Status and props failures are not JSON, so the page
 * reports no connection; generation failures follow the chat completion routes.
 *
 * @param {any} router
 * @param {{ safeInvoke: (command: import('../context/types.js').TauriInvokeCommand, args?: any) => Promise<any> }} context
 * @param {{ jsonResponse: (data: unknown, status?: number) => Response }} responses
 * @param {ReturnType<import('../services/ai/generation-sessions.js').createGenerationSessions>} generation
 */
export function registerTextCompletionRoutes(router, context, { jsonResponse }, generation) {
    router.post('/api/backends/text-completions/status', async (/** @type {{ body: unknown }} */ { body }) => {
        const status = await context.safeInvoke('get_text_completions_status', {
            dto: body,
            locale: getSillyTavernLocale(),
        });
        const response = jsonResponse({ result: status.result, data: status.data });
        if (status.supports_tokenization) {
            response.headers.set('x-supports-tokenization', 'true');
        }
        return response;
    });

    router.post('/api/backends/text-completions/props', async (/** @type {{ body: unknown }} */ { body }) => {
        return jsonResponse(await context.safeInvoke('get_text_completions_props', {
            dto: body,
            locale: getSillyTavernLocale(),
        }));
    });

    router.post('/api/backends/text-completions/ollama/download', async (/** @type {{ body: unknown }} */ { body }) => {
        await context.safeInvoke('download_ollama_model', {
            dto: body,
            locale: getSillyTavernLocale(),
        });
        return jsonResponse({ ok: true });
    });

    router.post('/api/backends/text-completions/tabby/download', async (/** @type {{ body: unknown }} */ { body }) => {
        await context.safeInvoke('download_tabby_model', {
            dto: body,
            locale: getSillyTavernLocale(),
        });
        return jsonResponse({ ok: true });
    });

    router.post('/api/backends/text-completions/generate', (
        /** @type {{ body: unknown; init?: RequestInit }} */ { body, init },
    ) => {
        const payload = copyGenerationPayload(body);
        const args = { dto: payload, locale: getSillyTavernLocale() };

        return generation.generate({
            payload,
            signal: init?.signal,
            chunkText: extractChunkText,
            startStream: (streamId) => context.safeInvoke('start_text_completion_stream', { ...args, streamId }),
            runRequest: () => generation.request({ command: 'generate_text_completion', args, signal: init?.signal }),
            errorChunk: buildErrorChunk,
            errorCompletion: buildErrorCompletion,
            quietErrorResponse: (error, errorMessage) => jsonResponse({
                error: true,
                status: commandErrorResponse(error).status,
                response: errorMessage,
            }),
            label: 'Text completion',
        });
    });
}
