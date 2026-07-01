import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { createTokenCountBroker } from '../src/tauri/main/brokers/token-count-broker.js';
import { registerOpenAiTokenizerRoutes } from '../src/tauri/main/routes/openai-tokenizer-routes.js';

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
