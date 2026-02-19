function asObject(value) {
    return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

function getErrorMessage(error) {
    if (!error) {
        return 'Unknown error';
    }

    if (typeof error === 'string') {
        return error;
    }

    return error.message || error.toString?.() || 'Unknown error';
}

const DEFAULT_COMPLETION_MODEL = 'tauritavern-error';
const DEFAULT_ERROR_MESSAGE = 'Chat completion request failed';
const ERROR_LABEL = '[API Error]';
const ERROR_PREFIX_PATTERNS = Object.freeze([
    /^internal server error:\s*/i,
    /^internal error:\s*/i,
    /^validation error:\s*/i,
    /^bad request:\s*/i,
    /^unauthorized:\s*/i,
    /^permission denied:\s*/i,
]);
const STREAM_FRAME_INTERVAL_MS = 10;
const STREAM_RESPONSE_HEADERS = Object.freeze({
    'Content-Type': 'text/event-stream; charset=utf-8',
    'Cache-Control': 'no-cache',
    Connection: 'keep-alive',
});

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
        source.zai_model,
    ];

    for (const candidate of candidates) {
        if (typeof candidate === 'string' && candidate.trim()) {
            return candidate.trim();
        }
    }

    return DEFAULT_COMPLETION_MODEL;
}

function stripKnownErrorPrefixes(message) {
    let normalized = String(message || '').trim();
    if (!normalized) {
        return '';
    }

    let previous = '';
    while (normalized && normalized !== previous) {
        previous = normalized;
        for (const prefixPattern of ERROR_PREFIX_PATTERNS) {
            normalized = normalized.replace(prefixPattern, '').trim();
        }
    }

    return normalized;
}

function buildErrorAssistantText(error) {
    const rawMessage = getErrorMessage(error);
    const normalizedMessage = stripKnownErrorPrefixes(rawMessage) || DEFAULT_ERROR_MESSAGE;
    if (normalizedMessage.startsWith(ERROR_LABEL)) {
        return normalizedMessage;
    }

    return `${ERROR_LABEL}\n${normalizedMessage}`;
}

function buildLegacyErrorPayload(error) {
    return {
        error: {
            message: getErrorMessage(error),
        },
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

async function createChatCompletionStreamResponse(context, payload, signal) {
    const streamId = createStreamId();
    const eventName = `chat-completion-stream:${streamId}`;
    const encoder = new TextEncoder();

    const tauriEvent = window.__TAURI__?.event;
    if (typeof tauriEvent?.listen !== 'function') {
        throw new Error('Tauri event API is unavailable');
    }

    let isClosed = false;
    let sawDone = false;
    let unlisten = null;
    let flushTimer = null;
    let abortHandler = null;
    let controllerRef = null;
    const pendingFrames = [];

    const flushFrames = () => {
        if (!controllerRef || pendingFrames.length === 0) {
            return;
        }

        const framed = pendingFrames.map((data) => `data: ${data}\n\n`).join('');
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

    const closeStream = async ({ cancelUpstream = false, appendDone = false, errorPayload = null } = {}) => {
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

        if (typeof unlisten === 'function') {
            try {
                unlisten();
            } catch {
                // ignore listener teardown failures
            }
            unlisten = null;
        }

        if (signal && abortHandler) {
            signal.removeEventListener('abort', abortHandler);
            abortHandler = null;
        }

        if (cancelUpstream) {
            try {
                await context.safeInvoke('cancel_chat_completion_stream', { streamId });
            } catch (error) {
                console.debug('Failed to cancel chat completion stream:', error);
            }
        }
    };

    const onStreamEvent = (event) => {
        if (isClosed) {
            return;
        }

        const eventPayload = asObject(event?.payload);
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

            scheduleFlush();
            return;
        }

        if (eventType === 'error') {
            const message = typeof eventPayload.message === 'string' && eventPayload.message.trim()
                ? eventPayload.message
                : 'Chat completion stream failed';
            void closeStream({
                appendDone: true,
                errorPayload: buildErrorStreamChunk(message, payload),
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
                unlisten = await tauriEvent.listen(eventName, onStreamEvent);
            } catch (error) {
                const message = getErrorMessage(error);
                await closeStream({
                    appendDone: true,
                    errorPayload: buildErrorStreamChunk(message, payload),
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
                });
            } catch (error) {
                const message = getErrorMessage(error);
                await closeStream({
                    appendDone: true,
                    errorPayload: buildErrorStreamChunk(message, payload),
                });
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
    router.post('/api/backends/chat-completions/status', async ({ body }) => {
        const payload = asObject(body);
        const dto = {
            chat_completion_source: String(payload.chat_completion_source || ''),
            reverse_proxy: String(payload.reverse_proxy || ''),
            proxy_password: String(payload.proxy_password || ''),
            custom_url: String(payload.custom_url || ''),
            custom_include_headers: String(payload.custom_include_headers || ''),
            bypass_status_check: Boolean(payload.bypass_status_check),
        };

        try {
            const result = await context.safeInvoke('get_chat_completions_status', { dto });
            return jsonResponse(result || { data: [] });
        } catch (error) {
            console.error('Chat completion status failed:', error);
            return jsonResponse(
                {
                    error: true,
                    message: getErrorMessage(error),
                    data: { data: [] },
                },
                200,
            );
        }
    });

    router.post('/api/backends/chat-completions/generate', async ({ body, init }) => {
        const payload = { ...asObject(body) };
        const wantsStream = Boolean(payload.stream);

        try {
            if (wantsStream) {
                return await createChatCompletionStreamResponse(context, payload, init?.signal);
            }

            const completion = await context.safeInvoke('generate_chat_completion', { dto: payload });
            return jsonResponse(completion || {});
        } catch (error) {
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

    router.post('/api/backends/chat-completions/bias', async ({ body, url }) => {
        const model = String(url?.searchParams?.get('model') || '');
        const entries = Array.isArray(body) ? body : [];
        const dto = { model, entries };

        try {
            const bias = await context.safeInvoke('build_openai_logit_bias', { dto });
            return jsonResponse(bias || {});
        } catch (error) {
            console.error('Failed to build logit bias:', error);
            return jsonResponse({});
        }
    });

    router.post('/api/tokenizers/openai/count', async ({ body, url }) => {
        const model = String(url?.searchParams?.get('model') || '');
        const messages = Array.isArray(body) ? body : [];
        const dto = { model, messages };

        try {
            const result = await context.safeInvoke('count_openai_tokens', { dto });
            return jsonResponse(result || { token_count: 0 });
        } catch (error) {
            console.error('OpenAI token count failed:', error);
            return jsonResponse({ token_count: 0 });
        }
    });

    router.post('/api/tokenizers/openai/encode', async ({ body, url }) => {
        const model = String(url?.searchParams?.get('model') || '');
        const payload = asObject(body);
        const dto = {
            model,
            text: String(payload.text || ''),
        };

        try {
            const result = await context.safeInvoke('encode_openai_tokens', { dto });
            return jsonResponse(result || { ids: [], count: 0, chunks: [] });
        } catch (error) {
            console.error('OpenAI token encode failed:', error);
            return jsonResponse({ ids: [], count: 0, chunks: [] });
        }
    });

    router.post('/api/tokenizers/openai/decode', async ({ body, url }) => {
        const model = String(url?.searchParams?.get('model') || '');
        const payload = asObject(body);
        const ids = Array.isArray(payload.ids)
            ? payload.ids
                .map((id) => Number(id))
                .filter((id) => Number.isInteger(id) && id >= 0)
            : [];

        const dto = { model, ids };

        try {
            const result = await context.safeInvoke('decode_openai_tokens', { dto });
            return jsonResponse(result || { text: '', chunks: [] });
        } catch (error) {
            console.error('OpenAI token decode failed:', error);
            return jsonResponse({ text: '', chunks: [] });
        }
    });
}
