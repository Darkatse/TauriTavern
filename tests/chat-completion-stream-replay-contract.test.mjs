import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';

function installBrowserShims() {
    globalThis.window ??= {};
    globalThis.document ??= {};
    Object.defineProperty(globalThis.document, 'visibilityState', {
        configurable: true,
        value: 'visible',
    });
    globalThis.document.hasFocus = () => true;
    globalThis.localStorage ??= {
        getItem: () => null,
        setItem: () => {},
        removeItem: () => {},
    };
}

test('chat completion stream advances afterSeq and releases the Rust session', async () => {
    installBrowserShims();
    const { registerAiRoutes } = await import('../src/tauri/main/routes/ai-routes.js');
    const router = createRouteRegistry();
    const calls = [];
    let releaseSession;
    const released = new Promise((resolve) => {
        releaseSession = resolve;
    });

    registerAiRoutes(router, {
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'start_chat_completion_stream') {
                assert.equal('onEvent' in args, false);
                return null;
            }
            if (command === 'read_chat_completion_stream') {
                if (args.afterSeq === 0) {
                    return {
                        events: [{ type: 'chunk', seq: 1, data: '{"choices":[]}' }],
                        status: 'running',
                    };
                }
                assert.equal(args.afterSeq, 1);
                return {
                    events: [{ type: 'done', seq: 2 }],
                    status: 'done',
                };
            }
            if (command === 'release_chat_completion_stream') {
                releaseSession();
                return null;
            }
            throw new Error(`Unexpected invoke: ${command}`);
        },
    }, { jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/backends/chat-completions/generate',
        body: { stream: true, type: 'quiet' },
        init: {},
    });

    assert.equal(response.status, 200);
    assert.equal(await response.text(), 'data: {"choices":[]}\n\ndata: [DONE]\n\n');
    await released;

    const streamCalls = calls.filter(({ command }) => command.includes('chat_completion_stream'));
    assert.deepEqual(streamCalls.map(({ command }) => command), [
        'start_chat_completion_stream',
        'read_chat_completion_stream',
        'read_chat_completion_stream',
        'release_chat_completion_stream',
    ]);
    assert.equal(streamCalls[0].args.streamId, streamCalls[3].args.streamId);
});
