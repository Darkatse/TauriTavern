import { mustGetSillyTavernContext } from '../adapters/st/active-chat-ref.js';
import { getSillyTavernLocale } from '../adapters/st/sillytavern-i18n.js';
import {
    buildLegacyErrorPayload,
    formatAssistantErrorText,
    getUpstreamFailureDetails,
    getUserFacingErrorMessage,
} from '../services/ai/ai-error-presenter.js';
import { registerCaptionRoutes } from './caption-routes.js';
import { createErrorStreamResponse, isRequestCancelled } from '../services/ai/generation-sessions.js';
import { createAbortError } from '../kernel/abort-error.js';
import { createRandomId } from '../kernel/random-id.js';

const DEFAULT_COMPLETION_MODEL = 'tauritavern-error';
const OPENCODE_STABLE_CHAT_ID_FIELD = '_tauritavern_stable_chat_id';

function asObject(value) {
    return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

function getChatCompletionSource(payload) {
    return String(asObject(payload).chat_completion_source || '').trim().toLowerCase();
}

function isQuietRequest(payload) {
    return String(asObject(payload).type || '').trim().toLowerCase() === 'quiet';
}

function getCompletionModel(payload) {
    const source = asObject(payload);
    const candidates = [
        source.model,
        source.openai_model,
        source.custom_model,
        source.claude_model,
        source.google_model,
        source.vertexai_model,
        source.deepseek_model,
        source.moonshot_model,
        source.siliconflow_model,
        source.minimax_model,
        source.aws_bedrock_model,
        source.zai_model,
    ];

    for (const candidate of candidates) {
        if (typeof candidate === 'string' && candidate.trim()) {
            return candidate.trim();
        }
    }

    return DEFAULT_COMPLETION_MODEL;
}

function isVertexAiClaudePayload(payload) {
    return getChatCompletionSource(payload) === 'vertexai'
        && getCompletionModel(payload).toLowerCase().startsWith('claude-');
}

function isOpenCodeFormat(payload, format) {
    return getChatCompletionSource(payload) === 'opencode'
        && String(asObject(payload).opencode_api_format || '').trim() === format;
}

async function attachOpenCodeStableChatId(payload) {
    if (getChatCompletionSource(payload) !== 'opencode') {
        return;
    }

    payload[OPENCODE_STABLE_CHAT_ID_FIELD] = mustGetSillyTavernContext().getCurrentChatId()
        ? await globalThis.__TAURITAVERN__.api.chat.current.handle().stableId()
        : createRandomId();
}

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
 * @param {unknown} data
 * @returns {string}
 */
function extractOpenAiChunkText(data) {
    const parsed = JSON.parse(String(data || ''));
    const delta = parsed?.choices?.[0]?.delta;
    return typeof delta?.content === 'string' ? delta.content : '';
}

async function requestChatCompletion(generation, payload, signal) {
    await attachOpenCodeStableChatId(payload);
    return generation.request({
        command: 'generate_chat_completion',
        args: { dto: payload, locale: getSillyTavernLocale() },
        signal,
    });
}

function streamChatCompletion(context, generation, payload, signal, lifecycle) {
    return generation.streamResponse({
        start: async (streamId) => {
            await attachOpenCodeStableChatId(payload);
            return context.safeInvoke('start_chat_completion_stream', {
                streamId,
                dto: payload,
                locale: getSillyTavernLocale(),
            });
        },
        errorChunk: (content) => buildErrorStreamChunk(content, payload),
        signal,
        lifecycle,
    });
}

/**
 * @param {ReturnType<import('../services/ai/generation-sessions.js').createGenerationSessions>} generation
 */
export function registerAiRoutes(router, context, { jsonResponse }, generation) {
    registerCaptionRoutes(router, {
        jsonResponse,
        invokeChatCompletion: (payload, signal) => requestChatCompletion(generation, payload, signal),
    });

    router.post('/api/backends/chat-completions/status', async ({ body }) => {
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

    router.post('/api/backends/chat-completions/generate', async ({ body, init }) => {
        const payload = { ...asObject(body) };
        const wantsStream = Boolean(payload.stream);
        const lifecycle = generation.createLifecycle({
            quiet: isQuietRequest(payload),
            chunkText: extractOpenAiChunkText,
        });
        lifecycle.begin();

        try {
            if (wantsStream) {
                return await streamChatCompletion(context, generation, payload, init?.signal, lifecycle);
            }

            const completion = await requestChatCompletion(generation, payload, init?.signal);
            lifecycle.finish({ success: true });
            return jsonResponse(completion || {});
        } catch (error) {
            const aborted = isRequestCancelled(error);
            const errorMessage = aborted ? '' : getUserFacingErrorMessage(error);

            lifecycle.finish({
                success: false,
                errorMessage,
                notifyFailure: !aborted,
            });

            if (aborted) {
                throw createAbortError();
            }

            console.error('Chat completion generation failed:', error);
            if (isQuietRequest(payload)) {
                return jsonResponse(buildLegacyErrorPayload(error), 502);
            }

            const assistantText = formatAssistantErrorText(errorMessage);
            if (wantsStream) {
                return createErrorStreamResponse(buildErrorStreamChunk(assistantText, payload));
            }

            return jsonResponse(buildErrorCompletionPayload(assistantText, payload));
        }
    });
}
