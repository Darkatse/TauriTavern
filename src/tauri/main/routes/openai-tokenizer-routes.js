import { createTokenCountBroker, trimOpenAiMessage } from '../brokers/token-count-broker.js';

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

export function registerOpenAiTokenizerRoutes(router, context, { jsonResponse }) {
    const tokenCountBroker = createTokenCountBroker({ context });

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
        if (!Array.isArray(body)) return jsonResponse({ error: 'OpenAI token count body must be an array' }, 400);
        try {
            return jsonResponse({ token_count: await tokenCountBroker.count({ model, messages: body }) });
        } catch (error) {
            console.warn('OpenAI token count failed:', error);
            return jsonResponse({ error: getErrorMessage(error) }, 500);
        }
    });

    router.post('/api/tokenizers/openai/count-batch', async ({ body, url }) => {
        const model = String(url?.searchParams?.get('model') || '');
        if (!Array.isArray(body)) return jsonResponse({ error: 'OpenAI token count batch body must be an array' }, 400);

        const dto = { model, requests: body.map((message) => ({ messages: [trimOpenAiMessage(message)] })) };

        try {
            return jsonResponse(await context.safeInvoke('count_openai_tokens_batch', { dto }));
        } catch (error) {
            console.warn('OpenAI token count batch failed:', error);
            return jsonResponse({ error: getErrorMessage(error) }, 500);
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

