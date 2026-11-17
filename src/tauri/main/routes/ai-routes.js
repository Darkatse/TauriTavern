// @ts-check

import { mustGetSillyTavernContext } from '../adapters/st/active-chat-ref.js';
import { getSillyTavernLocale } from '../adapters/st/sillytavern-i18n.js';
import {
    getUpstreamFailureDetails,
    getUserFacingErrorMessage,
} from '../services/ai/ai-error-presenter.js';
import { registerCaptionRoutes } from './caption-routes.js';
import { copyGenerationPayload, isRequestCancelled } from '../services/ai/generation-sessions.js';
import { createRandomId } from '../kernel/random-id.js';

const DEFAULT_COMPLETION_MODEL = 'tauritavern-error';
const OPENCODE_STABLE_CHAT_ID_FIELD = '_tauritavern_stable_chat_id';

/** @param {Record<string, unknown>} payload */
function getChatCompletionSource(payload) {
    return String(payload.chat_completion_source || '').trim().toLowerCase();
}

/** @param {Record<string, unknown>} payload */
function getCompletionModel(payload) {
    const candidates = [
        payload.model,
        payload.openai_model,
        payload.custom_model,
        payload.claude_model,
        payload.google_model,
        payload.vertexai_model,
        payload.deepseek_model,
        payload.moonshot_model,
        payload.siliconflow_model,
        payload.minimax_model,
        payload.aws_bedrock_model,
        payload.zai_model,
    ];

    for (const candidate of candidates) {
        if (typeof candidate === 'string' && candidate.trim()) {
            return candidate.trim();
        }
    }

    return DEFAULT_COMPLETION_MODEL;
}

/** @param {Record<string, unknown>} payload */
function isVertexAiClaudePayload(payload) {
    return getChatCompletionSource(payload) === 'vertexai'
        && getCompletionModel(payload).toLowerCase().startsWith('claude-');
}

/**
 * @param {Record<string, unknown>} payload
 * @param {string} format
 */
function isOpenCodeFormat(payload, format) {
    return getChatCompletionSource(payload) === 'opencode'
        && String(payload.opencode_api_format || '').trim() === format;
}

/** @param {Record<string, unknown>} payload */
async function attachOpenCodeStableChatId(payload) {
    if (getChatCompletionSource(payload) !== 'opencode') {
        return;
    }

    if (!mustGetSillyTavernContext().getCurrentChatId()) {
        payload[OPENCODE_STABLE_CHAT_ID_FIELD] = createRandomId();
        return;
    }

    const chatApi = (/** @type {Window} */ (/** @type {unknown} */ (globalThis))).__TAURITAVERN__?.api?.chat;
    if (!chatApi) {
        throw new Error('TauriTavern host ABI is not initialized');
    }
    payload[OPENCODE_STABLE_CHAT_ID_FIELD] = await chatApi.current.handle().stableId();
}

/**
 * @param {string} content
 * @param {Record<string, unknown>} payload
 */
function buildErrorCompletionPayload(content, payload) {
    const timestamp = Math.floor(Date.now() / 1000);

    return {
        id: `tauritavern-error-${timestamp}`,
        object: 'chat.completion',
        created: timestamp,
        model: getCompletionModel(payload),
        choices: [
            {
                index: 0,
                message: {
                    role: 'assistant',
                    content,
                },
                finish_reason: 'stop',
            },
        ],
        usage: {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        },
    };
}

/**
 * @param {string} content
 * @param {Record<string, unknown>} payload
 */
function buildOpenAiStyleErrorChunk(content, payload) {
    const timestamp = Math.floor(Date.now() / 1000);
    return {
        id: `tauritavern-error-chunk-${timestamp}`,
        object: 'chat.completion.chunk',
        created: timestamp,
        model: getCompletionModel(payload),
        choices: [
            {
                index: 0,
                delta: {
                    content,
                },
                finish_reason: null,
            },
        ],
    };
}

/**
 * @param {string} content
 * @param {Record<string, unknown>} payload
 */
function buildErrorStreamChunk(content, payload) {
    const source = getChatCompletionSource(payload);

    if (source === 'claude' || isVertexAiClaudePayload(payload) || isOpenCodeFormat(payload, 'claude_messages')) {
        return {
            delta: {
                text: content,
            },
        };
    }

    if (source === 'makersuite' || source === 'vertexai' || isOpenCodeFormat(payload, 'gemini')) {
        return {
            candidates: [
                {
                    index: 0,
                    content: {
                        parts: [{ text: content }],
                    },
                },
            ],
        };
    }

    if (source === 'cohere') {
        return {
            type: 'content-delta',
            delta: {
                message: {
                    content: {
                        text: content,
                    },
                },
            },
        };
    }

    return buildOpenAiStyleErrorChunk(content, payload);
}

/**
 * @param {string} data
 * @returns {string}
 */
function extractOpenAiChunkText(data) {
    const parsed = JSON.parse(data);
    const delta = parsed?.choices?.[0]?.delta;
    return typeof delta?.content === 'string' ? delta.content : '';
}

/**
 * @param {ReturnType<import('../services/ai/generation-sessions.js').createGenerationSessions>} generation
 * @param {Record<string, unknown>} payload
 * @param {AbortSignal | null | undefined} [signal]
 */
async function requestChatCompletion(generation, payload, signal) {
    await attachOpenCodeStableChatId(payload);
    return generation.request({
        command: 'generate_chat_completion',
        args: { dto: payload, locale: getSillyTavernLocale() },
        signal,
    });
}

/**
 * @param {any} router
 * @param {{ safeInvoke: (command: import('../context/types.js').TauriInvokeCommand, args?: any) => Promise<any> }} context
 * @param {{ jsonResponse: (data: unknown, status?: number) => Response }} responses
 * @param {ReturnType<import('../services/ai/generation-sessions.js').createGenerationSessions>} generation
 */
export function registerAiRoutes(router, context, { jsonResponse }, generation) {
    registerCaptionRoutes(router, {
        jsonResponse,
        invokeChatCompletion: (/** @type {unknown} */ payload, /** @type {AbortSignal | null | undefined} */ signal) => requestChatCompletion(
            generation,
            copyGenerationPayload(payload),
            signal,
        ),
    });

    router.post('/api/backends/chat-completions/status', async (/** @type {{ body: unknown }} */ { body }) => {
        try {
            const result = await context.safeInvoke('get_chat_completions_status', {
                dto: body,
                locale: getSillyTavernLocale(),
            });
            return jsonResponse(result);
        } catch (error) {
            if (isRequestCancelled(error)) {
                return jsonResponse({ cancelled: true, data: [] });
            }
            console.error('Chat completion status failed:', error);
            const details = getUpstreamFailureDetails(error);
            return jsonResponse(
                {
                    error: true,
                    message: getUserFacingErrorMessage(error),
                    ...(details ? {
                        code: details.code,
                        category: details.category,
                        message_key: details.messageKey,
                        ...(details.endpoint ? { endpoint: details.endpoint } : {}),
                    } : {}),
                    data: { data: [] },
                },
                200,
            );
        }
    });

    router.post('/api/backends/chat-completions/generate', (
        /** @type {{ body: unknown; init?: RequestInit }} */ { body, init },
    ) => {
        const payload = copyGenerationPayload(body);

        return generation.generate({
            payload,
            signal: init?.signal,
            chunkText: extractOpenAiChunkText,
            startStream: async (streamId) => {
                await attachOpenCodeStableChatId(payload);
                return context.safeInvoke('start_chat_completion_stream', {
                    streamId,
                    dto: payload,
                    locale: getSillyTavernLocale(),
                });
            },
            runRequest: () => requestChatCompletion(generation, payload, init?.signal),
            errorChunk: (content) => buildErrorStreamChunk(content, payload),
            errorCompletion: (content) => buildErrorCompletionPayload(content, payload),
            label: 'Chat completion',
        });
    });
}
