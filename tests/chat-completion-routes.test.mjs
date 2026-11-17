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
    const { createGenerationSessions } = await import('../src/tauri/main/services/ai/generation-sessions.js');
    const router = createRouteRegistry();
    const context = { safeInvoke };
    registerAiRoutes(router, context, { jsonResponse }, createGenerationSessions(context));
    return router;
}

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
    window.SillyTavern = {
        getContext: () => ({ getCurrentChatId: () => 'current-chat' }),
    };

    await router.handle({
        method: 'POST',
        path: '/api/backends/chat-completions/generate',
        body: { chat_completion_source: 'opencode', type: 'quiet' },
    });

    const generate = calls.find(call => call.command === 'generate_chat_completion');
    assert.equal(generate.args.dto._tauritavern_stable_chat_id, 'stable-chat');
});

test('pre-stream cancellation and quiet start failures are handled before returning SSE 200', async () => {
    const cancelledRouter = await createAiRouter(async (command) => {
        if (command === 'get_notification_permission_state') {
            return 'denied';
        }
        if (command === 'start_chat_completion_stream') {
            throw { Cancelled: 'Endpoint authorization cancelled by user' };
        }
        throw new Error(`Unexpected command: ${command}`);
    });

    await assert.rejects(
        cancelledRouter.handle({
            method: 'POST',
            path: '/api/backends/chat-completions/generate',
            body: { stream: true },
        }),
        { name: 'AbortError' },
    );

    const quietErrorRouter = await createAiRouter(async (command) => {
        if (command === 'start_chat_completion_stream') {
            throw { Forbidden: 'iOS policy disabled capability: llm.chat_completion_sources' };
        }
        throw new Error(`Unexpected command: ${command}`);
    });

    const originalConsoleError = console.error;
    console.error = () => {};
    let quietResponse;
    try {
        quietResponse = await quietErrorRouter.handle({
            method: 'POST',
            path: '/api/backends/chat-completions/generate',
            body: { stream: true, type: 'quiet' },
        });
    } finally {
        console.error = originalConsoleError;
    }

    assert.equal(quietResponse.status, 502);
    assert.deepEqual(await quietResponse.json(), {
        error: { message: 'iOS policy disabled capability: llm.chat_completion_sources' },
    });
});
