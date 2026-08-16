import { createTokenCountBroker } from '../brokers/token-count-broker.js';
import { getErrorMessage } from './ai-error-presenter.js';

const MAX_TOKEN_ID = 0xFFFF_FFFF;
const LOCAL_TOKENIZER_MODELS = Object.freeze([
    'gpt2',
    'llama',
    'nerdstash',
    'nerdstash_v2',
    'mistral',
    'yi',
    'claude',
    'llama3',
    'gemma',
    'jamba',
    'qwen2',
    'command-r',
    'command-a',
    'nemo',
    'deepseek',
]);

function asObject(value) {
    return value && typeof value === 'object' && !Array.isArray(value) ? value : null;
}

function registerCodecRoutes(router, context, jsonResponse, basePath, resolveModel) {
    router.post(`${basePath}/encode`, async ({ body, url }) => {
        const payload = asObject(body);
        if (!payload || (payload.text !== undefined && typeof payload.text !== 'string')) {
            return jsonResponse({ error: 'Tokenizer encode body must contain text as a string' }, 400);
        }

        const model = resolveModel(url);
        const dto = { model, text: payload.text ?? '' };

        try {
            return jsonResponse(await context.safeInvoke('encode_tokens', { dto }));
        } catch (error) {
            const message = getErrorMessage(error);
            console.error(`Failed to encode tokens with '${model}':`, error);
            return jsonResponse({ ids: [], count: 0, chunks: [], error: message });
        }
    });

    router.post(`${basePath}/decode`, async ({ body, url }) => {
        const payload = asObject(body);
        const ids = payload?.ids ?? [];
        if (!payload || !Array.isArray(ids) || ids.some(id => !Number.isSafeInteger(id) || id < 0 || id > MAX_TOKEN_ID)) {
            return jsonResponse({ error: 'Tokenizer decode body must contain non-negative 32-bit integer ids' }, 400);
        }

        const model = resolveModel(url);
        const dto = { model, ids };

        try {
            return jsonResponse(await context.safeInvoke('decode_tokens', { dto }));
        } catch (error) {
            const message = getErrorMessage(error);
            console.error(`Failed to decode tokens with '${model}':`, error);
            return jsonResponse({ text: '', chunks: [], error: message });
        }
    });
}

export function registerTokenizerRoutes(router, context, { jsonResponse }) {
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

        const dto = { model, requests: body.map(message => ({ messages: [message] })) };

        try {
            return jsonResponse(await context.safeInvoke('count_openai_tokens_batch', { dto }));
        } catch (error) {
            console.warn('OpenAI token count batch failed:', error);
            return jsonResponse({ error: getErrorMessage(error) }, 500);
        }
    });

    router.post('/api/tokenizers/openai/count-prefix-batch', async ({ body, url }) => {
        const model = String(url?.searchParams?.get('model') || '');
        const payload = asObject(body);
        if (!payload || typeof payload.base !== 'string' || !Array.isArray(payload.suffixes) || payload.suffixes.some(suffix => typeof suffix !== 'string')) {
            return jsonResponse({ error: 'OpenAI token prefix count body must contain a string base and string suffixes' }, 400);
        }

        const stopAt = Number.isSafeInteger(payload.stop_at) && payload.stop_at >= 0 ? payload.stop_at : null;
        const dto = { model, base: payload.base, suffixes: payload.suffixes, stop_at: stopAt };
        try {
            return jsonResponse(await context.safeInvoke('count_openai_token_prefixes', { dto }));
        } catch (error) {
            console.warn('OpenAI token prefix count failed:', error);
            return jsonResponse({ error: getErrorMessage(error) }, 500);
        }
    });

    registerCodecRoutes(
        router,
        context,
        jsonResponse,
        '/api/tokenizers/openai',
        url => String(url?.searchParams?.get('model') || ''),
    );

    for (const model of LOCAL_TOKENIZER_MODELS) {
        registerCodecRoutes(
            router,
            context,
            jsonResponse,
            `/api/tokenizers/${model}`,
            () => model,
        );
    }

    const remoteUnavailable = () => jsonResponse({
        error: 'Remote tokenizer APIs require native text-completion backend support',
    }, 501);
    router.post('/api/tokenizers/remote/kobold/count', remoteUnavailable);
    router.post('/api/tokenizers/remote/textgenerationwebui/encode', remoteUnavailable);
}
