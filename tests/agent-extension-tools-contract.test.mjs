import test from 'node:test';
import assert from 'node:assert/strict';
import { createAgentToolsApi } from '../src/tauri/main/api/agent-tools.js';

const definition = {
    extensionId: 'third-party/example',
    name: 'inspect',
    description: 'Inspect application state.',
    inputSchema: { type: 'object' },
    contexts: ['chat', 'session'],
};

function harness() {
    const channels = [];
    const replies = [];
    const api = createAgentToolsApi({
        channelFactory(onmessage) {
            const channel = { onmessage };
            channels.push(channel);
            return channel;
        },
        async safeInvoke(command, args) {
            if (command === 'resolve_agent_extension_tool_call') {
                // Exercise the same JSON serialization boundary as ordinary IPC.
                replies.push(JSON.parse(JSON.stringify(args.dto)));
                return true;
            }
        },
    });
    const call = (requestId, args = {}) => ({
        type: 'call', requestId,
        call: { toolId: 'extension/third-party/example:inspect', runId: 'run-1', invocationId: 'inv-1', callId: 'call-1', target: { kind: 'session', sessionId: 'session-1' }, arguments: args },
    });
    return { api, channels, replies, call };
}

test('registered extension callbacks await async closures before returning JSON results', async () => {
    const { api, channels, replies, call } = harness();
    let multiplier = 2;
    const ready = Promise.withResolvers();
    await api.register(definition, async (args) => {
        await ready.promise;
        return { answer: args.value * multiplier };
    });
    const completed = channels[0].onmessage(call('request-1', { value: 7 }));
    assert.deepEqual(replies, []);
    multiplier = 3;
    ready.resolve();
    await completed;
    assert.deepEqual(replies, [{ requestId: 'request-1', result: { kind: 'value', value: { answer: 21 } } }]);
});

test('undefined returns null while callback and JSON serialization failures return tool errors', async () => {
    const { api, channels, replies, call } = harness();
    const cyclic = {};
    cyclic.self = cyclic;
    await api.register(definition, ({ mode }) => {
        if (mode === 'throw') throw new Error('tool failed');
        if (mode === 'cyclic') return cyclic;
    });
    for (const mode of ['void', 'throw', 'cyclic']) {
        await channels[0].onmessage(call(mode, { mode }));
    }
    assert.deepEqual(replies[0].result, { kind: 'value', value: null });
    assert.deepEqual(replies[1].result, { kind: 'error', message: 'tool failed' });
    assert.equal(replies[2].result.kind, 'error');
});

test('cancellation aborts only its request and suppresses late completion', async () => {
    const { api, channels, replies, call } = harness();
    const blocked = Promise.withResolvers();
    const contexts = [];
    await api.register(definition, async (_, context) => {
        contexts.push(context);
        await blocked.promise;
        return 'complete';
    });
    const first = channels[0].onmessage(call('first'));
    const second = channels[0].onmessage(call('second'));
    channels[0].onmessage({ type: 'cancel', requestId: 'first' });
    assert.equal(contexts[0].signal.aborted, true);
    assert.equal(contexts[1].signal.aborted, false);
    blocked.resolve();
    await Promise.all([first, second]);
    assert.deepEqual(replies, [{ requestId: 'second', result: { kind: 'value', value: 'complete' } }]);
});
