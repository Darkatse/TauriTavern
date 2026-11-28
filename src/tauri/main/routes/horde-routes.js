// @ts-check

import { copyGenerationPayload } from '../services/ai/generation-sessions.js';

/**
 * A finished Horde status whose generation is `text`, so a failure reads as the reply.
 * @param {string} text
 */
function buildErrorStatus(text) {
    return { done: true, generations: [{ text }] };
}

/**
 * @param {string} data
 * @returns {string}
 */
function extractGenerationText(data) {
    const text = JSON.parse(data)?.generations?.[0]?.text;
    return typeof text === 'string' ? text : '';
}

/**
 * @param {unknown} body
 * @returns {boolean}
 */
function isForced(body) {
    return Boolean(/** @type {{ force?: unknown } | null} */ (body)?.force);
}

/**
 * @param {any} router
 * @param {{ safeInvoke: (command: import('../context/types.js').TauriInvokeCommand, args?: any) => Promise<any> }} context
 * @param {{ jsonResponse: (data: unknown, status?: number) => Response }} responses
 * @param {ReturnType<import('../services/ai/generation-sessions.js').createGenerationSessions>} generation
 */
export function registerHordeRoutes(router, context, { jsonResponse }, generation) {
    router.post('/api/horde/status', async () => {
        await context.safeInvoke('check_horde_status');
        return jsonResponse({ ok: true });
    });

    router.post('/api/horde/text-workers', async (/** @type {{ body: unknown }} */ { body }) => {
        return jsonResponse(await context.safeInvoke('get_horde_text_workers', { force: isForced(body) }));
    });

    router.post('/api/horde/text-models', async (/** @type {{ body: unknown }} */ { body }) => {
        return jsonResponse(await context.safeInvoke('get_horde_text_models', { force: isForced(body) }));
    });

    router.post('/api/horde/user-info', async () => {
        return jsonResponse(await context.safeInvoke('get_horde_user_info'));
    });

    router.post('/api/backends/koboldhorde/generate', (
        /** @type {{ body: unknown; init?: RequestInit }} */ { body, init },
    ) => {
        const payload = copyGenerationPayload(body);

        return generation.generate({
            payload,
            signal: init?.signal,
            chunkText: extractGenerationText,
            startStream: (streamId) => context.safeInvoke('start_horde_text_stream', { streamId, dto: payload }),
            errorChunk: buildErrorStatus,
            label: 'Horde',
        });
    });
}
