import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { createTokenCountBroker } from '../src/tauri/main/brokers/token-count-broker.js';
import { registerOpenAiTokenizerRoutes } from '../src/tauri/main/routes/openai-tokenizer-routes.js';

test('OpenAI token prefix count command is part of the centralized invoke contract', async () => {
    const source = await readFile(new URL('../src/tauri/main/kernel/invokes/tauri-commands.js', import.meta.url), 'utf8');

    assert.match(source, /\| 'count_openai_token_prefixes'/);
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

    const messages = [
        {
            role: 'user',
            content: 'hello',
            custom_payload: { weighted: true },
        },
    ];

    assert.equal(await broker.count({ model: 'gpt-4o', messages }), 42);
    assert.deepEqual(capturedDto.requests[0].messages[0], messages[0]);
});

test('OpenAI token count batch route preserves all message fields', async () => {
    let capturedDto;
    const router = createRouteRegistry();
    registerOpenAiTokenizerRoutes(
        router,
        {
            async safeInvoke(command, { dto }) {
                assert.equal(command, 'count_openai_tokens_batch');
                capturedDto = dto;
                return { token_counts: [7] };
            },
        },
        { jsonResponse },
    );

    const message = {
        role: 'assistant',
        content: 'hi',
        experimental_field: ['kept'],
    };
    const response = await router.handle({
        method: 'POST',
        path: '/api/tokenizers/openai/count-batch',
        url: new URL('http://tauri.local/api/tokenizers/openai/count-batch?model=gpt-4o'),
        body: [message],
    });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { token_counts: [7] });
    assert.deepEqual(capturedDto.requests[0].messages[0], message);
});

test('OpenAI token count batch route still invokes backend for empty warm batch', async () => {
    let capturedDto;
    const router = createRouteRegistry();
    registerOpenAiTokenizerRoutes(
        router,
        {
            async safeInvoke(command, { dto }) {
                assert.equal(command, 'count_openai_tokens_batch');
                capturedDto = dto;
                return { token_counts: [] };
            },
        },
        { jsonResponse },
    );

    const response = await router.handle({
        method: 'POST',
        path: '/api/tokenizers/openai/count-batch',
        url: new URL('http://tauri.local/api/tokenizers/openai/count-batch?model=gpt-4o'),
        body: [],
    });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { token_counts: [] });
    assert.deepEqual(capturedDto, { model: 'gpt-4o', requests: [] });
});

test('OpenAI token prefix count route preserves compact prefix parts', async () => {
    let capturedDto;
    const router = createRouteRegistry();
    registerOpenAiTokenizerRoutes(
        router,
        {
            async safeInvoke(command, { dto }) {
                assert.equal(command, 'count_openai_token_prefixes');
                capturedDto = dto;
                return { token_counts: [8, 13] };
            },
        },
        { jsonResponse },
    );

    const response = await router.handle({
        method: 'POST',
        path: '/api/tokenizers/openai/count-prefix-batch',
        url: new URL('http://tauri.local/api/tokenizers/openai/count-prefix-batch?model=gpt-4o'),
        body: { base: 'base', suffixes: [' one', ' two'], stop_at: 12 },
    });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { token_counts: [8, 13] });
    assert.deepEqual(capturedDto, { model: 'gpt-4o', base: 'base', suffixes: [' one', ' two'], stop_at: 12 });
});

test('OpenAI token prefix count route rejects invalid prefix parts', async () => {
    let invokeCount = 0;
    const router = createRouteRegistry();
    registerOpenAiTokenizerRoutes(
        router,
        {
            async safeInvoke() {
                invokeCount += 1;
                throw new Error('safeInvoke should not run for an invalid request body');
            },
        },
        { jsonResponse },
    );

    const response = await router.handle({
        method: 'POST',
        path: '/api/tokenizers/openai/count-prefix-batch',
        url: new URL('http://tauri.local/api/tokenizers/openai/count-prefix-batch?model=gpt-4o'),
        body: { base: 42, suffixes: ['valid', 7] },
    });

    assert.equal(response.status, 400);
    assert.deepEqual(await response.json(), {
        error: 'OpenAI token prefix count body must contain a string base and string suffixes',
    });
    assert.equal(invokeCount, 0);
});
