import { estimateTokenCount } from '../brokers/token-count-broker.js';
import { createAndroidGenerationBridge } from '../adapters/android/android-generation-bridge.js';
import { confirmAiNotificationPermissionRationale } from '../adapters/st/ai-notification-permission-rationale-popup.js';
import { translateSillyTavern } from '../adapters/st/sillytavern-i18n.js';
import { createGenerationLifecycleService } from '../services/ai/generation-lifecycle-service.js';
import { createGenerationStatusBridge } from '../services/ai/generation-status-bridge.js';
import { createSystemNotificationService } from '../services/notifications/system-notification-service.js';
import { registerOpenAiTokenizerRoutes } from './openai-tokenizer-routes.js';
import {
    asUpstreamFailureDetails,
    getErrorMessage,
    getUpstreamFailureDetails,
    getUserFacingErrorMessage,
    translateApiErrorLabel,
} from './ai-error-presenter.js';
import { createChannel } from '../../../tauri-bridge.js';
import { stripCommandErrorPrefixes } from '../../../scripts/util/command-error-utils.js';
import { createAbortError, isAbortError } from '../kernel/abort-error.js';

function asObject(value) {
    return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

function encodeSseDataFrame(data) {
    const text = typeof data === 'string' ? data : String(data ?? '');
    if (!text.includes('\n') && !text.includes('\r')) {
        return `data: ${text}\n\n`;
    }

    const lines = text.split(/\r\n|\r|\n/g);
    const framed = lines.map((line) => `data: ${line}\n`).join('');
    return `${framed}\n`;
}

const DEFAULT_COMPLETION_MODEL = 'tauritavern-error';
const DEFAULT_ERROR_MESSAGE = 'Chat completion request failed';
const STREAM_FRAME_INTERVAL_MS = 10;
const STREAM_RESPONSE_HEADERS = Object.freeze({
    'Content-Type': 'text/event-stream; charset=utf-8',
    'Cache-Control': 'no-cache',
    Connection: 'keep-alive',
});
const ANDROID_GENERATION_BRIDGE_NAME = 'TauriTavernAndroidAiBridge';
const FAILURE_NOTIFICATION_MAX_BODY_LENGTH = 180;
const ANDROID_LIVE_UPDATE_TOKEN_THROTTLE_MS = 4000;
const ANDROID_LIVE_UPDATE_TOKEN_MIN_CHARS_DELTA = 160;
const CAPTION_UNAVAILABLE_ROUTES = Object.freeze([
    '/api/extra/caption',
    '/api/horde/caption-image',
    '/api/openai/caption-image',
    '/api/google/caption-image',
    '/api/anthropic/caption-image',
    '/api/backends/text-completions/ollama/caption-image',
]);
const CAPTION_UNAVAILABLE_MESSAGE = 'Image captioning is not implemented in the TauriTavern native backend.';
const i18nNotificationKeys = Object.freeze({
    successTitle: 'tauritavern_ai_notification_success_title',
    successBody: 'tauritavern_ai_notification_success_body',
    failureTitle: 'tauritavern_ai_notification_failure_title',
    failureBody: 'tauritavern_ai_notification_failure_body',
});
const i18nNotificationFallbacks = Object.freeze({
    successTitle: 'AI reply is ready',
    successBody: 'Tap to return to TauriTavern',
    failureTitle: 'AI reply failed',
    failureBody: 'Generation failed. Tap to return to TauriTavern',
});

const androidGenerationBridge = createAndroidGenerationBridge({ bridgeName: ANDROID_GENERATION_BRIDGE_NAME });
const generationStatusBridge = createGenerationStatusBridge({ bridge: androidGenerationBridge });

function extractHttpStatusCode(errorMessage) {
    const text = String(errorMessage || '');
    const explicit = text.match(/\b(?:status|http)\s*[:=]?\s*(\d{3})\b/i);
    if (explicit) {
        const value = Number(explicit[1]);
        if (Number.isInteger(value) && value >= 400 && value <= 599) {
            return value;
        }
    }

    const common = text.match(/\b(429|503)\b/);
    if (common) {
        return Number(common[1]);
    }

    return 0;
}

function shouldNotifyCompletion() {
    if (document.visibilityState === 'hidden') {
        return true;
    }

    if (typeof document.hasFocus === 'function') {
        try {
            return !document.hasFocus();
        } catch {
            return false;
        }
    }

    return false;
}

function translateNotificationText(key, fallback) {
    return translateSillyTavern(key, fallback);
}

function getGenerationNotificationTexts() {
    return {
        successTitle: translateNotificationText(i18nNotificationKeys.successTitle, i18nNotificationFallbacks.successTitle),
        successBody: translateNotificationText(i18nNotificationKeys.successBody, i18nNotificationFallbacks.successBody),
        failureTitle: translateNotificationText(i18nNotificationKeys.failureTitle, i18nNotificationFallbacks.failureTitle),
        failureBody: translateNotificationText(i18nNotificationKeys.failureBody, i18nNotificationFallbacks.failureBody),
    };
}

function pickFirstStringValue(source) {
    if (typeof source === 'string') {
        const value = source.trim();
        return value || null;
    }

    if (!source || typeof source !== 'object') {
        return null;
    }

    if (Array.isArray(source)) {
        for (const item of source) {
            const nested = pickFirstStringValue(item);
            if (nested) {
                return nested;
            }
        }

        return null;
    }

    for (const value of Object.values(source)) {
        const nested = pickFirstStringValue(value);
        if (nested) {
            return nested;
        }
    }

    return null;
}

function normalizeFailureNotificationBody(errorMessage) {
    const raw = String(errorMessage || '').trim();
    let normalized = stripCommandErrorPrefixes(raw);

    if (normalized.startsWith('{') && normalized.endsWith('}')) {
        try {
            const parsed = JSON.parse(normalized);
            const parsedMessage = pickFirstStringValue(parsed);
            if (parsedMessage) {
                normalized = stripCommandErrorPrefixes(parsedMessage);
            }
        } catch {
            // Keep original normalized text.
        }
    }

    if (!normalized) {
        return '';
    }

    const statusCode = extractHttpStatusCode(normalized);
    if (statusCode) {
        return `Error ${statusCode}`;
    }

    if (normalized.length > FAILURE_NOTIFICATION_MAX_BODY_LENGTH) {
        return `${normalized.slice(0, FAILURE_NOTIFICATION_MAX_BODY_LENGTH - 3)}...`;
    }

    return normalized;
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

function buildErrorAssistantText(error) {
    const normalizedMessage = getUserFacingErrorMessage(error);
    const errorLabel = translateApiErrorLabel();
    if (normalizedMessage.startsWith(errorLabel) || normalizedMessage.startsWith('[API Error]')) {
        return normalizedMessage;
    }

    return `${errorLabel}\n${normalizedMessage}`;
}

function buildLegacyErrorPayload(error) {
    const details = getUpstreamFailureDetails(error);
    const payload = {
        message: getUserFacingErrorMessage(error),
    };

    if (details) {
        payload.code = details.code;
        payload.category = details.category;
        payload.message_key = details.messageKey;
        if (details.endpoint) {
            payload.endpoint = details.endpoint;
        }
    }

    return {
        error: payload,
    };
}

function buildErrorCompletionPayload(error, payload) {
    const timestamp = Math.floor(Date.now() / 1000);
    const content = buildErrorAssistantText(error);

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

function buildOpenAiStyleErrorChunk(error, payload) {
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
                    content: buildErrorAssistantText(error),
                },
                finish_reason: null,
            },
        ],
    };
}

function buildErrorStreamChunk(error, payload) {
    const content = buildErrorAssistantText(error);
    const source = getChatCompletionSource(payload);

    if (source === 'claude') {
        return {
            delta: {
                text: content,
            },
        };
    }

    if (source === 'makersuite' || source === 'vertexai') {
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

    return buildOpenAiStyleErrorChunk(error, payload);
}

function createImmediateErrorStreamResponse(error, payload) {
    const encoder = new TextEncoder();
    const chunk = buildErrorStreamChunk(error, payload);
    const readable = new ReadableStream({
        start(controller) {
            controller.enqueue(encoder.encode(`data: ${JSON.stringify(chunk)}\n\n`));
            controller.enqueue(encoder.encode('data: [DONE]\n\n'));
            controller.close();
        },
    });

    return new Response(readable, {
        status: 200,
        headers: STREAM_RESPONSE_HEADERS,
    });
}

function createStreamId() {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
        return crypto.randomUUID();
    }

    const timestamp = Date.now().toString(36);
    const random = Math.random().toString(36).slice(2, 10);
    return `${timestamp}-${random}`;
}

async function invokeChatCompletionWithAbort(context, payload, signal) {
    if (signal?.aborted) {
        throw createAbortError();
    }

    const requestId = createStreamId();
    let abortRequested = false;
    let abortHandler = null;
    if (signal) {
        abortHandler = () => {
            abortRequested = true;
            void context.safeInvoke('cancel_chat_completion_generation', { requestId })
                .catch((error) => {
                    console.debug('Failed to cancel chat completion generation:', error);
                });
        };
        signal.addEventListener('abort', abortHandler, { once: true });
    }

    try {
        const result = await context.safeInvoke('generate_chat_completion', {
            requestId,
            dto: payload,
        });

        if (abortRequested) {
            throw createAbortError();
        }

        return result;
    } finally {
        if (signal && abortHandler) {
            signal.removeEventListener('abort', abortHandler);
        }
    }
}

async function createChatCompletionStreamResponse(context, payload, signal, lifecycle) {
    const streamId = createStreamId();
    const encoder = new TextEncoder();

    let isClosed = false;
    let sawDone = false;
    let channel = null;
    let flushTimer = null;
    let abortHandler = null;
    let controllerRef = null;
    let streamStartSettled = false;
    let cancelAfterStart = false;
    const pendingFrames = [];

    const requestUpstreamCancel = async () => {
        try {
            await context.safeInvoke('cancel_chat_completion_stream', { streamId });
        } catch (error) {
            console.debug('Failed to cancel chat completion stream:', error);
        }
    };

    const flushFrames = () => {
        if (!controllerRef || pendingFrames.length === 0) {
            return;
        }

        const framed = pendingFrames.map((data) => encodeSseDataFrame(data)).join('');
        pendingFrames.length = 0;
        controllerRef.enqueue(encoder.encode(framed));
    };

    const scheduleFlush = () => {
        if (flushTimer !== null || isClosed) {
            return;
        }

        flushTimer = setTimeout(() => {
            flushTimer = null;
            flushFrames();
        }, STREAM_FRAME_INTERVAL_MS);
    };

    const closeStream = async ({
        cancelUpstream = false,
        appendDone = false,
        errorPayload = null,
        failureMessage = '',
    } = {}) => {
        if (isClosed) {
            return;
        }

        isClosed = true;

        if (flushTimer !== null) {
            clearTimeout(flushTimer);
            flushTimer = null;
        }

        if (errorPayload) {
            pendingFrames.push(JSON.stringify(errorPayload));
        }

        if (appendDone && !sawDone) {
            sawDone = true;
            pendingFrames.push('[DONE]');
        }

        flushFrames();

        if (controllerRef) {
            try {
                controllerRef.close();
            } catch {
                // stream already closed
            }
        }

        if (signal && abortHandler) {
            signal.removeEventListener('abort', abortHandler);
            abortHandler = null;
        }

        if (channel) {
            channel.onmessage = () => {};
            channel = null;
        }

        if (cancelUpstream) {
            if (!streamStartSettled) {
                cancelAfterStart = true;
            }

            await requestUpstreamCancel();
        }

        const isSuccessfulCompletion = sawDone && !cancelUpstream && !errorPayload;
        const shouldNotifyFailure = !isSuccessfulCompletion && !cancelUpstream && Boolean(failureMessage || errorPayload);
        await lifecycle?.finish({
            success: isSuccessfulCompletion,
            errorMessage: failureMessage,
            notifyFailure: shouldNotifyFailure,
        });
    };

    const onStreamEvent = (message) => {
        if (isClosed) {
            return;
        }

        const eventPayload = asObject(message);
        const eventType = String(eventPayload.type || '');

        if (eventType === 'chunk') {
            const data = typeof eventPayload.data === 'string' ? eventPayload.data : '';
            if (!data) {
                return;
            }

            pendingFrames.push(data);

            if (data === '[DONE]') {
                sawDone = true;
                flushFrames();
                void closeStream();
                return;
            }

            lifecycle?.reportStreamChunk(data);

            scheduleFlush();
            return;
        }

        if (eventType === 'error') {
            const rawMessage = typeof eventPayload.message === 'string' && eventPayload.message.trim()
                ? eventPayload.message
                : 'Chat completion stream failed';
            const error = {
                message: rawMessage,
                details: asUpstreamFailureDetails(eventPayload.details),
            };
            const message = getUserFacingErrorMessage(error);
            void closeStream({
                appendDone: true,
                errorPayload: buildErrorStreamChunk(error, payload),
                failureMessage: message,
            });
            return;
        }

        if (eventType === 'done') {
            void closeStream({ appendDone: true });
        }
    };

    const readable = new ReadableStream({
        async start(controller) {
            controllerRef = controller;

            try {
                channel = createChannel(onStreamEvent);
            } catch (error) {
                const message = getUserFacingErrorMessage(error);
                await closeStream({
                    appendDone: true,
                    errorPayload: buildErrorStreamChunk(message, payload),
                    failureMessage: message,
                });
                return;
            }

            if (signal) {
                abortHandler = () => {
                    void closeStream({ cancelUpstream: true });
                };

                if (signal.aborted) {
                    abortHandler();
                    return;
                }

                signal.addEventListener('abort', abortHandler, { once: true });
            }

            try {
                await context.safeInvoke('start_chat_completion_stream', {
                    streamId,
                    dto: payload,
                    onEvent: channel,
                });

                // If abort happened while stream registration was in-flight, run cancellation again
                // after start settles to avoid a missed pre-registration cancel race.
                if (cancelAfterStart) {
                    cancelAfterStart = false;
                    await requestUpstreamCancel();
                }
            } catch (error) {
                const message = getUserFacingErrorMessage(error);
                await closeStream({
                    appendDone: true,
                    errorPayload: buildErrorStreamChunk(message, payload),
                    failureMessage: message,
                });
            } finally {
                streamStartSettled = true;
            }
        },
        async cancel() {
            await closeStream({ cancelUpstream: true });
        },
    });

    return new Response(readable, {
        status: 200,
        headers: STREAM_RESPONSE_HEADERS,
    });
}

export function registerAiRoutes(router, context, { jsonResponse }) {
    const notificationService = createSystemNotificationService({
        safeInvoke: context.safeInvoke,
        confirmPermissionRationale: confirmAiNotificationPermissionRationale,
    });
    const generationLifecycleService = createGenerationLifecycleService({
        notificationService,
        statusBridge: generationStatusBridge,
        shouldNotifyCompletion,
        getNotificationTexts: getGenerationNotificationTexts,
        normalizeFailureNotificationBody,
        extractFailureStatusCode: extractHttpStatusCode,
        estimateTokenCount,
        progressThrottleMs: ANDROID_LIVE_UPDATE_TOKEN_THROTTLE_MS,
        progressMinCharsDelta: ANDROID_LIVE_UPDATE_TOKEN_MIN_CHARS_DELTA,
    });

    for (const route of CAPTION_UNAVAILABLE_ROUTES) {
        router.post(route, async () => jsonResponse({
            error: true,
            message: CAPTION_UNAVAILABLE_MESSAGE,
        }, 501));
    }

    router.post('/api/backends/chat-completions/status', async ({ body }) => {
        const payload = asObject(body);
        const dto = {
            chat_completion_source: String(payload.chat_completion_source || ''),
            custom_api_format: String(payload.custom_api_format || ''),
            reverse_proxy: String(payload.reverse_proxy || ''),
            proxy_password: String(payload.proxy_password || ''),
            custom_url: String(payload.custom_url || ''),
            custom_include_headers: payload.custom_include_headers ?? null,
            siliconflow_endpoint: String(payload.siliconflow_endpoint || ''),
            minimax_endpoint: String(payload.minimax_endpoint || ''),
            workers_ai_account_id: String(payload.workers_ai_account_id || ''),
            aws_bedrock_region: String(payload.aws_bedrock_region || ''),
            secret_id: payload.secret_id ?? null,
            bypass_status_check: Boolean(payload.bypass_status_check),
        };

        try {
            const result = await context.safeInvoke('get_chat_completions_status', { dto });
            return jsonResponse(result || { data: [] });
        } catch (error) {
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
        const lifecycle = generationLifecycleService.createLifecycle({
            quiet: isQuietRequest(payload),
        });
        lifecycle.begin();

        try {
            if (wantsStream) {
                return await createChatCompletionStreamResponse(context, payload, init?.signal, lifecycle);
            }

            const completion = await invokeChatCompletionWithAbort(context, payload, init?.signal);
            await lifecycle.finish({ success: true });
            return jsonResponse(completion || {});
        } catch (error) {
            const rawErrorMessage = getErrorMessage(error);
            const errorMessage = getUserFacingErrorMessage(error);
            const aborted = isAbortError(error)
                || /generation cancelled by user/i.test(rawErrorMessage);

            await lifecycle.finish({
                success: false,
                errorMessage: aborted ? '' : errorMessage,
                notifyFailure: !aborted,
            });

            if (aborted) {
                throw createAbortError();
            }

            console.error('Chat completion generation failed:', error);
            const isQuiet = isQuietRequest(payload);

            if (wantsStream) {
                if (isQuiet) {
                    return jsonResponse(buildLegacyErrorPayload(error), 502);
                }

                return createImmediateErrorStreamResponse(error, payload);
            }

            if (isQuiet) {
                return jsonResponse(buildLegacyErrorPayload(error), 502);
            }

            return jsonResponse(buildErrorCompletionPayload(error, payload));
        }
    });

    registerOpenAiTokenizerRoutes(router, context, { jsonResponse });
}
