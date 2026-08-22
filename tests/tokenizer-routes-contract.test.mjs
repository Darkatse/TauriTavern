import assert from 'node:assert/strict';
import test from 'node:test';

import { createTokenCountBroker } from '../src/tauri/main/brokers/token-count-broker.js';
import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerTokenizerRoutes } from '../src/tauri/main/routes/tokenizer-routes.js';

const LOCAL_TOKENIZERS = [
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
];

function createTokenizerRouter(context) {
    const router = createRouteRegistry();
    registerTokenizerRoutes(router, context, { jsonResponse });
    return router;
}

function tokenizerRequest(path, body, query = '') {
    return {
        method: 'POST',
        path,
        url: new URL(`http://tauri.local${path}${query}`),
        body,
    };
}




test('OpenAI batch route preserves message fields and warms an empty batch', async () => {
    const capturedDtos = [];
    const router = createTokenizerRouter({
        async safeInvoke(command, { dto }) {
            assert.equal(command, 'count_openai_tokens_batch');
            capturedDtos.push(dto);
            return { token_counts: dto.requests.map(() => 7) };
        },
    });
    const message = {
        role: 'assistant',
        content: 'hi',
        experimental_field: ['kept'],
    };

    const response = await router.handle(tokenizerRequest(
        '/api/tokenizers/openai/count-batch',
        [message],
        '?model=gpt-4o',
    ));
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { token_counts: [7] });
    assert.deepEqual(capturedDtos[0].requests[0].messages[0], message);

    const warmResponse = await router.handle(tokenizerRequest(
        '/api/tokenizers/openai/count-batch',
        [],
        '?model=gpt-4o',
    ));
    assert.equal(warmResponse.status, 200);
    assert.deepEqual(await warmResponse.json(), { token_counts: [] });
    assert.deepEqual(capturedDtos[1], { model: 'gpt-4o', requests: [] });
});

test('OpenAI prefix route preserves compact parts and rejects invalid bodies', async () => {
    let capturedDto;
    let invokeCount = 0;
    const router = createTokenizerRouter({
        async safeInvoke(command, { dto }) {
            assert.equal(command, 'count_openai_token_prefixes');
            invokeCount += 1;
            capturedDto = dto;
            return { token_counts: [8, 13] };
        },
    });

    const response = await router.handle(tokenizerRequest(
        '/api/tokenizers/openai/count-prefix-batch',
        { base: 'base', suffixes: [' one', ' two'], stop_at: 12 },
        '?model=gpt-4o',
    ));
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { token_counts: [8, 13] });
    assert.deepEqual(capturedDto, {
        model: 'gpt-4o',
        base: 'base',
        suffixes: [' one', ' two'],
        stop_at: 12,
    });

    const invalidResponse = await router.handle(tokenizerRequest(
        '/api/tokenizers/openai/count-prefix-batch',
        { base: 42, suffixes: ['valid', 7] },
        '?model=gpt-4o',
    ));
    assert.equal(invalidResponse.status, 400);
    assert.deepEqual(await invalidResponse.json(), {
        error: 'OpenAI token prefix count body must contain a string base and string suffixes',
    });
    assert.equal(invokeCount, 1);
});



test('Tokenizer codec routes reject invalid text and token ids before invoke', async () => {
    let invokeCount = 0;
    const router = createTokenizerRouter({
        async safeInvoke() {
            invokeCount += 1;
            throw new Error('invalid requests must not invoke Rust');
        },
    });

    const encodeResponse = await router.handle(tokenizerRequest(
        '/api/tokenizers/gpt2/encode',
        { text: 42 },
    ));
    assert.equal(encodeResponse.status, 400);

    const decodeResponse = await router.handle(tokenizerRequest(
        '/api/tokenizers/gpt2/decode',
        { ids: [0, -1, 0x1_0000_0000] },
    ));
    assert.equal(decodeResponse.status, 400);
    assert.equal(invokeCount, 0);
});

test('Tokenizer operational failures preserve upstream empty results and expose the error', async () => {
    const router = createTokenizerRouter({
        async safeInvoke() {
            throw new Error('model unavailable');
        },
    });

    const response = await router.handle(tokenizerRequest(
        '/api/tokenizers/nerdstash_v2/encode',
        { text: 'hello' },
    ));
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
        ids: [],
        count: 0,
        chunks: [],
        error: 'model unavailable',
    });
});
