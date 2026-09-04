import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';

async function createAiRouter(safeInvoke) {
    globalThis.window ??= {};
    globalThis.document ??= { visibilityState: 'visible' };
    globalThis.localStorage ??= {
        getItem: () => null,
        setItem: () => {},
        removeItem: () => {},
    };

    const { registerAiRoutes } = await import('../src/tauri/main/routes/ai-routes.js');
    const router = createRouteRegistry();
    registerAiRoutes(router, { safeInvoke }, { jsonResponse });
    return router;
}

test('chat completion status preserves the selected credential identity', async () => {
    const calls = [];
    const router = await createAiRouter(async (command, args) => {
        calls.push({ command, args });
        return { data: [] };
    });
    const customHeaders = {
        'Content-Type': 'application/json',
        Authorization: 'Bearer proxy-secret',
    };

    const response = await router.handle({
        method: 'POST',
        path: '/api/backends/chat-completions/status',
        body: {
            chat_completion_source: 'custom',
            secret_id: 'profile-secret',
            custom_include_headers: customHeaders,
        },
    });

    assert.equal(response.status, 200);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].command, 'get_chat_completions_status');
    assert.equal(calls[0].args.dto.secret_id, 'profile-secret');
    assert.deepEqual(calls[0].args.dto.custom_include_headers, customHeaders);
});

test('chat completion status exposes structured network failures', async () => {
    const router = await createAiRouter(async () => {
        const error = new Error('error sending request for url (https://api.example.test/v1/chat/completions)');
        error.details = {
            code: 'network.proxy_failed',
            category: 'network',
            endpoint: 'https://api.example.test/v1/chat/completions',
            messageKey: 'tauritavern.error.network.proxy_failed',
        };
        throw error;
    });

    const originalConsoleError = console.error;
    console.error = () => {};
    let response;
    try {
        response = await router.handle({
            method: 'POST',
            path: '/api/backends/chat-completions/status',
            body: { chat_completion_source: 'openai' },
        });
    } finally {
        console.error = originalConsoleError;
    }

    const body = await response.json();
    assert.deepEqual({
        status: response.status,
        error: body.error,
        code: body.code,
        category: body.category,
        messageKey: body.message_key,
        endpoint: body.endpoint,
    }, {
        status: 200,
        error: true,
        code: 'network.proxy_failed',
        category: 'network',
        messageKey: 'tauritavern.error.network.proxy_failed',
        endpoint: 'https://api.example.test/v1/chat/completions',
    });
    assert.match(body.message, /Could not connect through the configured proxy/);
});

test('OpenCode generation carries the current stable chat id', async () => {
    globalThis.__TAURITAVERN__ = {
        api: {
            chat: {
                current: {
                    handle: () => ({ stableId: async () => 'stable-chat' }),
                },
            },
        },
    };
    const calls = [];
    const router = await createAiRouter(async (command, args) => {
        calls.push({ command, args });
        return {};
    });

    await router.handle({
        method: 'POST',
        path: '/api/backends/chat-completions/generate',
        body: { chat_completion_source: 'opencode', type: 'quiet' },
    });

    const generate = calls.find(call => call.command === 'generate_chat_completion');
    assert.equal(generate.args.dto._tauritavern_stable_chat_id, 'stable-chat');
});
