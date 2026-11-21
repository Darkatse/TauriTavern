import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';

async function createTextCompletionRouter(safeInvoke) {
    globalThis.window ??= {};
    globalThis.document ??= { visibilityState: 'visible' };

    const { registerTextCompletionRoutes } = await import('../src/tauri/main/routes/text-completion-routes.js');
    const { createGenerationSessions } = await import('../src/tauri/main/services/ai/generation-sessions.js');
    const router = createRouteRegistry();
    const context = { safeInvoke };
    registerTextCompletionRoutes(router, context, { jsonResponse }, createGenerationSessions(context));
    return router;
}

async function withoutConsoleErrors(run) {
    const originalConsoleError = console.error;
    console.error = () => {};
    try {
        return await run();
    } finally {
        console.error = originalConsoleError;
    }
}

test('status projects supports_tokenization into the response header', async () => {
    let supportsTokenization = true;
    const router = await createTextCompletionRouter(async (command) => {
        if (command === 'get_text_completions_status') {
            return {
                result: 'ooba-model',
                data: [{ id: 'ooba-model' }],
                supports_tokenization: supportsTokenization,
            };
        }
        throw new Error(`Unexpected command: ${command}`);
    });

    const withHeader = await router.handle({
        method: 'POST',
        path: '/api/backends/text-completions/status',
        body: { api_type: 'ooba', api_server: 'http://127.0.0.1:5000' },
    });
    assert.equal(withHeader.status, 200);
    assert.equal(withHeader.headers.get('x-supports-tokenization'), 'true');
    assert.deepEqual(await withHeader.json(), {
        result: 'ooba-model',
        data: [{ id: 'ooba-model' }],
    });

    supportsTokenization = false;
    const withoutHeader = await router.handle({
        method: 'POST',
        path: '/api/backends/text-completions/status',
        body: { api_type: 'llamacpp', api_server: 'http://127.0.0.1:8080' },
    });
    assert.equal(withoutHeader.status, 200);
    assert.equal(withoutHeader.headers.get('x-supports-tokenization'), null);
});

test('a failed generation is an assistant reply, and a quiet one keeps the upstream error shape', async () => {
    const router = await createTextCompletionRouter(async (command) => {
        if (command === 'get_generation_notification_state') {
            return { enabled: false, backgroundShowsProgressUi: false };
        }
        if (command === 'generate_text_completion') {
            throw { Unauthorized: 'KoboldCpp endpoint rejected the API key' };
        }
        if (command === 'start_text_completion_stream') {
            throw { Forbidden: 'iOS policy disabled capability: llm.text_completions.enabled' };
        }
        throw new Error(`Unexpected command: ${command}`);
    });
    const generate = (body) => withoutConsoleErrors(() => router.handle({
        method: 'POST',
        path: '/api/backends/text-completions/generate',
        body: { api_type: 'koboldcpp', api_server: 'http://127.0.0.1:5001', ...body },
    }));

    const reply = await generate({ type: 'normal' });
    assert.equal(reply.status, 200);
    assert.deepEqual(await reply.json(), {
        choices: [{
            index: 0,
            text: '[API Error]\nKoboldCpp endpoint rejected the API key',
            finish_reason: 'stop',
        }],
    });

    const quiet = await generate({ type: 'quiet' });
    assert.equal(quiet.status, 200);
    assert.deepEqual(await quiet.json(), {
        error: true,
        status: 401,
        response: 'KoboldCpp endpoint rejected the API key',
    });

    const quietStream = await generate({ type: 'quiet', stream: true });
    assert.equal(quietStream.status, 502);
    assert.deepEqual(await quietStream.json(), {
        error: { message: 'iOS policy disabled capability: llm.text_completions.enabled' },
    });
});

test('streaming generation forwards chunks and ends mid-stream errors with an error chunk and DONE', async () => {
    let closed = false;
    const router = await createTextCompletionRouter(async (command) => {
        if (command === 'get_generation_notification_state') {
            return { enabled: false, backgroundShowsProgressUi: false };
        }
        if (command === 'start_text_completion_stream') {
            return null;
        }
        if (command === 'read_generation_stream') {
            return {
                events: [
                    { type: 'chunk', seq: 1, data: '{"choices":[{"index":0,"text":"Hi"}]}' },
                    { type: 'error', seq: 2, message: 'Ollama: model ran out of memory' },
                ],
                status: 'error',
            };
        }
        if (command === 'close_generation_stream') {
            closed = true;
            return null;
        }
        throw new Error(`Unexpected command: ${command}`);
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/backends/text-completions/generate',
        body: { api_type: 'ollama', api_server: 'http://127.0.0.1:11434', stream: true, type: 'normal' },
    });
    assert.equal(response.status, 200);
    const body = await response.text();
    assert.equal(
        body,
        'data: {"choices":[{"index":0,"text":"Hi"}]}\n\n'
        + 'data: {"choices":[{"index":0,"text":"[API Error]\\nOllama: model ran out of memory"}]}\n\n'
        + 'data: [DONE]\n\n',
    );
    assert.equal(closed, true);
});
