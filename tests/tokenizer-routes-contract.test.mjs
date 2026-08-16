import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
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

test('Tokenizer commands are part of the centralized invoke contract', async () => {
    const source = await readFile(new URL('../src/tauri/main/kernel/invokes/tauri-commands.js', import.meta.url), 'utf8');

    assert.match(source, /\| 'count_openai_token_prefixes'/);
    assert.match(source, /\| 'encode_tokens'/);
    assert.match(source, /\| 'decode_tokens'/);
    assert.doesNotMatch(source, /\| '(?:encode|decode)_openai_tokens'/);
});

test('OpenAI token count broker preserves all message fields', async () => {
    let capturedDto;
    const broker = createTokenCountBroker({
        flushIntervalMs: 0,
        context: {
            async safeInvoke(command, { dto }) {
                assert.equal(command, 'count_openai_tokens_batch');
                capturedDto = dto;
                return { token_counts: [42] };
            },
        },
    });

    const messages = [{
        role: 'user',
        content: 'hello',
        custom_payload: { weighted: true },
    }];

    assert.equal(await broker.count({ model: 'gpt-4o', messages }), 42);
    assert.deepEqual(capturedDto.requests[0].messages[0], messages[0]);
});

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

test('All SillyTavern local tokenizer codec routes use their exact canonical model', async () => {
    const calls = [];
    const router = createTokenizerRouter({
        async safeInvoke(command, { dto }) {
            calls.push({ command, dto });
            return command === 'encode_tokens'
                ? { ids: [7], count: 1, chunks: ['x'] }
                : { text: 'x', chunks: ['x'] };
        },
    });

    for (const model of LOCAL_TOKENIZERS) {
        const encodePath = `/api/tokenizers/${model}/encode`;
        const encodeResponse = await router.handle(tokenizerRequest(encodePath, { text: 'x' }));
        assert.equal(encodeResponse.status, 200, encodePath);
        assert.deepEqual(await encodeResponse.json(), { ids: [7], count: 1, chunks: ['x'] });
        assert.deepEqual(calls.at(-1), {
            command: 'encode_tokens',
            dto: { model, text: 'x' },
        });

        const decodePath = `/api/tokenizers/${model}/decode`;
        const decodeResponse = await router.handle(tokenizerRequest(decodePath, { ids: [7] }));
        assert.equal(decodeResponse.status, 200, decodePath);
        assert.deepEqual(await decodeResponse.json(), { text: 'x', chunks: ['x'] });
        assert.deepEqual(calls.at(-1), {
            command: 'decode_tokens',
            dto: { model, ids: [7] },
        });
    }
});

test('OpenAI codec routes preserve the requested model', async () => {
    const calls = [];
    const router = createTokenizerRouter({
        async safeInvoke(command, { dto }) {
            calls.push({ command, dto });
            return { ids: [], count: 0, chunks: [] };
        },
    });

    const response = await router.handle(tokenizerRequest(
        '/api/tokenizers/openai/encode',
        { text: '' },
        '?model=llama-3.3-70b',
    ));
    assert.equal(response.status, 200);
    assert.deepEqual(calls, [{
        command: 'encode_tokens',
        dto: { model: 'llama-3.3-70b', text: '' },
    }]);
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

test('Remote tokenizer routes fail explicitly until text-completion transport exists', async () => {
    const router = createTokenizerRouter({
        async safeInvoke() {
            throw new Error('remote tokenizer routes must not invoke Rust');
        },
    });

    for (const path of [
        '/api/tokenizers/remote/kobold/count',
        '/api/tokenizers/remote/textgenerationwebui/encode',
    ]) {
        const response = await router.handle(tokenizerRequest(path, { text: 'hello' }));
        assert.equal(response.status, 501, path);
        assert.deepEqual(await response.json(), {
            error: 'Remote tokenizer APIs require native text-completion backend support',
        });
    }
});
