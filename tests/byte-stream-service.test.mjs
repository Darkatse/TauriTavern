import assert from 'node:assert/strict';
import test from 'node:test';

import { createByteStreamService } from '../src/tauri/main/services/files/byte-stream-service.js';

async function readStreamBytes(stream) {
    return new Uint8Array(await new Response(stream).arrayBuffer());
}

function mockByteService(overrides = {}) {
    const calls = [];
    const commands = {
        'plugin:resources|close': () => null,
        ...overrides,
    };
    const service = createByteStreamService({
        invoke: async (command, args) => {
            calls.push({ command, args });
            assert.ok(commands[command], 'Unexpected command: ' + command);
            return commands[command](args);
        },
    });
    return { service, calls };
}

const closes = calls => calls.filter(call => call.command === 'plugin:resources|close');

test('byte stream delivers host chunks until an empty read, then closes once', async () => {
    const chunks = [Uint8Array.of(1, 2).buffer, Uint8Array.of(3), new Uint8Array(0)];
    const { service, calls } = mockByteService({ read_bytes: () => chunks.shift() });

    assert.deepEqual(await readStreamBytes(service.createByteStream(7)), Uint8Array.of(1, 2, 3));
    assert.deepEqual(closes(calls), [{ command: 'plugin:resources|close', args: { rid: 7 } }]);
    assert.equal(calls.at(-1).command, 'plugin:resources|close');
});

test('byte stream closes the resource when a read fails', async t => {
    for (const [name, read, error] of [
        ['read rejection', () => { throw new Error('read failed'); }, /read failed/],
        ['non-binary response', () => [65, 66], /Unexpected IPC byte response/],
    ]) {
        await t.test(name, async () => {
            const { service, calls } = mockByteService({ read_bytes: read });
            await assert.rejects(readStreamBytes(service.createByteStream(7)), error);
            assert.deepEqual(closes(calls), [{ command: 'plugin:resources|close', args: { rid: 7 } }]);
        });
    }
});

test('read failure retains both the original and cleanup errors', async () => {
    const failure = new Error('read failed');
    const closeFailure = new Error('close failed');
    const { service, calls } = mockByteService({
        read_bytes: () => { throw failure; },
        'plugin:resources|close': () => { throw closeFailure; },
    });
    await assert.rejects(readStreamBytes(service.createByteStream(7)), error => {
        assert.ok(error instanceof AggregateError);
        assert.deepEqual(error.errors, [failure, closeFailure]);
        return true;
    });
    assert.equal(closes(calls).length, 1);
});

test('close failure is not a successful end of stream', async () => {
    const failure = new Error('close failed');
    const { service, calls } = mockByteService({
        read_bytes: () => new Uint8Array(0),
        'plugin:resources|close': () => { throw failure; },
    });
    await assert.rejects(readStreamBytes(service.createByteStream(7)), error => error === failure);
    assert.equal(closes(calls).length, 1);
});

test('cancelling during a read closes once and delivers no data', async () => {
    const entered = Promise.withResolvers();
    const pending = Promise.withResolvers();
    const { service, calls } = mockByteService({
        read_bytes: () => {
            entered.resolve();
            return pending.promise;
        },
    });
    const reader = service.createByteStream(7).getReader();
    const reading = reader.read();
    await entered.promise;
    const cancelled = reader.cancel();
    pending.resolve(Uint8Array.of(1, 2, 3, 4));
    await cancelled;

    assert.deepEqual(await reading, { value: undefined, done: true });
    assert.deepEqual(closes(calls), [{ command: 'plugin:resources|close', args: { rid: 7 } }]);
});
