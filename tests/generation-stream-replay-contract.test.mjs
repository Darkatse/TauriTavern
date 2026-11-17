import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { consumeGenerationStream } from '../src/tauri/main/services/ai/generation-stream-consumer.js';

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

test('generation stream retries one failed cursor read', async () => {
    const cursors = [];
    const status = await consumeGenerationStream({
        streamId: 'stream-1',
        isClosed: () => false,
        onEvent: () => {},
        safeInvoke: async (_command, { afterSeq }) => {
            cursors.push(afterSeq);
            if (cursors.length === 1) {
                throw new Error('temporary invoke failure');
            }
            return { events: [], status: 'done' };
        },
    });

    assert.equal(status, 'done');
    assert.deepEqual(cursors, [0, 0]);

    let failedReads = 0;
    await assert.rejects(
        consumeGenerationStream({
            streamId: 'stream-2',
            isClosed: () => false,
            onEvent: () => {},
            safeInvoke: async () => {
                failedReads += 1;
                throw new Error('persistent invoke failure');
            },
        }),
        /persistent invoke failure/,
    );
    assert.equal(failedReads, 2);
});

test('generation stream advances afterSeq and closes the Rust session', async () => {
    installBrowserShims();
    const { registerAiRoutes } = await import('../src/tauri/main/routes/ai-routes.js');
    const { createGenerationSessions } = await import('../src/tauri/main/services/ai/generation-sessions.js');
    const router = createRouteRegistry();
    const calls = [];
    let closeSession;
    const closed = new Promise((resolve) => {
        closeSession = resolve;
    });

    const context = {
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'start_chat_completion_stream') {
                assert.equal('onEvent' in args, false);
                return null;
            }
            if (command === 'read_generation_stream') {
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
            if (command === 'close_generation_stream') {
                closeSession();
                return null;
            }
            throw new Error(`Unexpected invoke: ${command}`);
        },
    };
    registerAiRoutes(router, context, { jsonResponse }, createGenerationSessions(context));

    const response = await router.handle({
        method: 'POST',
        path: '/api/backends/chat-completions/generate',
        body: { stream: true, type: 'quiet' },
        init: {},
    });

    assert.equal(response.status, 200);
    assert.equal(await response.text(), 'data: {"choices":[]}\n\ndata: [DONE]\n\n');
    await closed;

    const streamCalls = calls.filter(({ command }) => command.includes('_stream'));
    assert.deepEqual(streamCalls.map(({ command }) => command), [
        'start_chat_completion_stream',
        'read_generation_stream',
        'read_generation_stream',
        'close_generation_stream',
    ]);
    assert.equal(streamCalls[0].args.streamId, streamCalls[3].args.streamId);
});
