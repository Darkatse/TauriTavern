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

function normalizeMessageContent(content) {
    if (typeof content === 'string') {
        return content;
    }

    if (Array.isArray(content)) {
        return content
            .map((item) => {
                if (typeof item === 'string') {
                    return item;
                }

                if (item && typeof item === 'object') {
                    if (typeof item.text === 'string') {
                        return item.text;
                    }

                    if (typeof item.content === 'string') {
                        return item.content;
                    }
                }

                return '';
            })
            .join('');
    }

    if (content === null || content === undefined) {
        return '';
    }

    return String(content);
}

function toSingleChunkSseResponse(completion) {
    const choice = completion?.choices?.[0] || {};
    const message = choice?.message || {};
    const content = normalizeMessageContent(message?.content);
    const delta = {};

    if (content) {
        delta.content = content;
    }

    if (Array.isArray(message?.tool_calls) && message.tool_calls.length > 0) {
        delta.tool_calls = message.tool_calls;
    }

    const chunk = {
        id: completion?.id || 'tauri-chat-completion',
        object: 'chat.completion.chunk',
        created: completion?.created || Math.floor(Date.now() / 1000),
        model: completion?.model || '',
        choices: [
            {
                index: 0,
                delta,
                finish_reason: choice?.finish_reason || (delta.tool_calls ? 'tool_calls' : 'stop'),
            },
        ],
    };

    const streamText = `data: ${JSON.stringify(chunk)}\n\ndata: [DONE]\n\n`;

    return new Response(streamText, {
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

    router.post('/api/backends/chat-completions/generate', async ({ body }) => {
        const payload = { ...asObject(body) };
        const wantsStream = Boolean(payload.stream);

        if (wantsStream) {
            payload.stream = false;
        }

        try {
            const completion = await context.safeInvoke('generate_chat_completion', { dto: payload });

            if (wantsStream) {
                return toSingleChunkSseResponse(completion);
            }

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
