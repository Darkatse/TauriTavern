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

const STREAM_FRAME_INTERVAL_MS = 10;

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

    const closeStream = async ({ cancelUpstream = false, appendDone = false, errorMessage = '' } = {}) => {
        if (isClosed) {
            return;
        }

        isClosed = true;

        if (flushTimer !== null) {
            clearTimeout(flushTimer);
            flushTimer = null;
        }

        if (errorMessage) {
            pendingFrames.push(JSON.stringify({ error: { message: errorMessage } }));
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

        const payload = asObject(event?.payload);
        const eventType = String(payload.type || '');

        if (eventType === 'chunk') {
            const data = typeof payload.data === 'string' ? payload.data : '';
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
            const message = typeof payload.message === 'string' && payload.message.trim()
                ? payload.message
                : 'Chat completion stream failed';
            void closeStream({ appendDone: true, errorMessage: message });
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
                await closeStream({ appendDone: true, errorMessage: message });
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
                await closeStream({ appendDone: true, errorMessage: message });
            }
        },
        async cancel() {
            await closeStream({ cancelUpstream: true });
        },
    });

    return new Response(readable, {
        status: 200,
        headers: {
            'Content-Type': 'text/event-stream; charset=utf-8',
            'Cache-Control': 'no-cache',
            Connection: 'keep-alive',
        },
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
            return jsonResponse(
                {
                    error: {
                        message: getErrorMessage(error),
                    },
                },
                502,
            );
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
