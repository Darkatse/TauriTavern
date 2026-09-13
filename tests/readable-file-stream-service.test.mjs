import assert from 'node:assert/strict';
import test from 'node:test';

import { createReadableFileStreamService } from '../src/tauri/main/services/files/readable-file-stream-service.js';

function createFsReadResponse(payload, requestedLength = payload.byteLength) {
    const response = new Uint8Array(requestedLength + 8);
    response.set(payload);
    new DataView(response.buffer).setBigUint64(requestedLength, BigInt(payload.byteLength));
    return response;
}

async function readStreamBytes(stream) {
    return new Uint8Array(await new Response(stream).arrayBuffer());
}

function mockFileService(overrides = {}) {
    const calls = [];
    const commands = {
        'plugin:fs|open': () => 7,
        'plugin:fs|fstat': () => ({ size: 4 }),
        'plugin:resources|close': () => null,
        ...overrides,
    };
    const service = createReadableFileStreamService({
        invoke: async (command, args) => {
            calls.push({ command, args });
            assert.ok(commands[command], 'Unexpected command: ' + command);
            return commands[command](args);
        },
    });
    return { service, calls };
}

test('file stream caps requests and reads the exact remaining bytes without an EOF call', async () => {
    const cap = 4 * 1024 * 1024;
    const payload = new Uint8Array(cap + 2).fill(0x61);
    payload.set([0x62, 0x63], cap);
    let offset = 0;
    const { service, calls } = mockFileService({
        'plugin:fs|fstat': () => ({ size: payload.length }),
        'plugin:fs|read': ({ len }) => {
            const chunk = payload.subarray(offset, offset + len);
            offset += chunk.length;
            return createFsReadResponse(chunk, len).buffer;
        },
    });

    assert.deepEqual(await readStreamBytes(service.createReadableFileStream('/tmp/archive.zip')), payload);
    assert.deepEqual(calls, [
        { command: 'plugin:fs|open', args: { path: '/tmp/archive.zip', options: { read: true } } },
        { command: 'plugin:fs|fstat', args: { rid: 7 } },
        { command: 'plugin:fs|read', args: { rid: 7, len: cap } },
        { command: 'plugin:fs|read', args: { rid: 7, len: 2 } },
        { command: 'plugin:resources|close', args: { rid: 7 } },
    ]);
});

test('file stream continues positive short reads without delivering padding', async () => {
    const chunks = [Uint8Array.of(1, 2), Uint8Array.of(3, 4, 5)];
    const { service, calls } = mockFileService({
        'plugin:fs|fstat': () => ({ size: 5 }),
        'plugin:fs|read': ({ len }) => createFsReadResponse(chunks.shift(), len),
    });

    assert.deepEqual(await readStreamBytes(service.createReadableFileStream('/tmp/file')), Uint8Array.of(1, 2, 3, 4, 5));
    assert.deepEqual(calls.filter(call => call.command === 'plugin:fs|read').map(call => call.args.len), [5, 3]);
});

test('empty file closes without a read', async () => {
    const { service, calls } = mockFileService({
        'plugin:fs|fstat': () => ({ size: 0 }),
    });

    assert.deepEqual(await readStreamBytes(service.createReadableFileStream('/tmp/empty')), new Uint8Array(0));
    assert.deepEqual(calls.map(call => call.command), ['plugin:fs|open', 'plugin:fs|fstat', 'plugin:resources|close']);
});

test('file stream fails instead of completing a truncated file', async () => {
    const chunks = [Uint8Array.of(1, 2), new Uint8Array(0)];
    const { service, calls } = mockFileService({
        'plugin:fs|read': ({ len }) => createFsReadResponse(chunks.shift(), len),
    });

    await assert.rejects(
        readStreamBytes(service.createReadableFileStream('/tmp/truncated')),
        /shorter than its declared size: \/tmp\/truncated \(2 bytes remaining\)/,
    );
    assert.equal(calls.filter(call => call.command === 'plugin:resources|close').length, 1);
});

test('file stream closes an opened resource when stat or read fails', async t => {
    const cases = [
        ...[undefined, null, '4', -1, 0.5, Number.MAX_SAFE_INTEGER + 1].map(size => ({
            name: 'invalid size ' + size,
            commands: { 'plugin:fs|fstat': () => ({ size }) },
            error: /Invalid file size/,
        })),
        {
            name: 'stat rejection',
            commands: { 'plugin:fs|fstat': () => { throw new Error('stat denied'); } },
            error: /stat denied/,
        },
        {
            name: 'read rejection',
            commands: { 'plugin:fs|read': () => { throw new Error('read failed'); } },
            error: /read failed/,
        },
        {
            name: 'non-binary response',
            commands: { 'plugin:fs|read': () => ({ bytes: [] }) },
            error: /Unexpected resource read response/,
        },
        {
            name: 'missing trailer',
            commands: { 'plugin:fs|read': () => Uint8Array.of(65, 66) },
            error: RangeError,
        },
        {
            name: 'read count exceeds request',
            commands: { 'plugin:fs|read': () => createFsReadResponse(Uint8Array.of(1, 2, 3, 4, 5)) },
            error: /Invalid fs read length/,
        },
        {
            name: 'read count exceeds response body',
            commands: { 'plugin:fs|read': () => createFsReadResponse(Uint8Array.of(1, 2, 3, 4)).subarray(1) },
            error: /Invalid fs read length/,
        },
    ];

    for (const entry of cases) {
        await t.test(entry.name, async () => {
            const { service, calls } = mockFileService(entry.commands);
            await assert.rejects(readStreamBytes(service.createReadableFileStream('/tmp/file')), entry.error);
            assert.deepEqual(calls.filter(call => call.command === 'plugin:resources|close'), [
                { command: 'plugin:resources|close', args: { rid: 7 } },
            ]);
            assert.equal(calls.at(-1).command, 'plugin:resources|close');
            if (entry.commands['plugin:fs|fstat']) {
                assert.equal(calls.some(call => call.command === 'plugin:fs|read'), false);
            }
        });
    }
});

test('open failure propagates without closing an unallocated resource', async () => {
    const failure = new Error('open denied');
    const { service, calls } = mockFileService({
        'plugin:fs|open': () => { throw failure; },
    });
    await assert.rejects(readStreamBytes(service.createReadableFileStream('/tmp/file')), error => error === failure);
    assert.deepEqual(calls.map(call => call.command), ['plugin:fs|open']);
});

test('read failure retains both the original and cleanup errors', async t => {
    const failure = new Error('operation failed');
    const closeFailure = new Error('close failed');
    for (const command of ['plugin:fs|fstat', 'plugin:fs|read']) {
        await t.test(command, async () => {
            const { service, calls } = mockFileService({
                [command]: () => { throw failure; },
                'plugin:resources|close': () => { throw closeFailure; },
            });
            await assert.rejects(readStreamBytes(service.createReadableFileStream('/tmp/file')), error => {
                assert.ok(error instanceof AggregateError);
                assert.deepEqual(error.errors, [failure, closeFailure]);
                return true;
            });
            assert.equal(calls.filter(call => call.command === 'plugin:resources|close').length, 1);
        });
    }
});

test('close failure is not a successful end of stream', async () => {
    const failure = new Error('close failed');
    const { service, calls } = mockFileService({
        'plugin:fs|fstat': () => ({ size: 0 }),
        'plugin:resources|close': () => { throw failure; },
    });
    await assert.rejects(readStreamBytes(service.createReadableFileStream('/tmp/file')), error => error === failure);
    assert.equal(calls.filter(call => call.command === 'plugin:resources|close').length, 1);
});

test('cancelling during open, stat or read closes once and delivers no data', async t => {
    for (const [command, result] of [
        ['plugin:fs|open', 7],
        ['plugin:fs|fstat', { size: 4 }],
        ['plugin:fs|read', createFsReadResponse(Uint8Array.of(1, 2, 3, 4))],
    ]) {
        await t.test(command, async () => {
            const entered = Promise.withResolvers();
            const pending = Promise.withResolvers();
            const { service, calls } = mockFileService({
                'plugin:fs|read': () => createFsReadResponse(Uint8Array.of(1, 2, 3, 4)),
                [command]: () => {
                    entered.resolve();
                    return pending.promise;
                },
            });
            const reader = service.createReadableFileStream('/tmp/file').getReader();
            const reading = reader.read();
            await entered.promise;
            const cancelled = reader.cancel();
            pending.resolve(result);
            await cancelled;

            assert.deepEqual(await reading, { value: undefined, done: true });
            assert.deepEqual(calls.filter(call => call.command === 'plugin:resources|close'), [
                { command: 'plugin:resources|close', args: { rid: 7 } },
            ]);
        });
    }
});

test('chat backup stream retains host-owned chunking without fs stat', async () => {
    const chunks = [Uint8Array.of(1, 2), Uint8Array.of(3), new Uint8Array(0)];
    const { service, calls } = mockFileService({
        'open_chat_backup_download': () => 8,
        'read_chat_backup_download': () => chunks.shift(),
    });
    const stream = await service.createChatBackupDownloadStream('chat_alice.jsonl');

    assert.deepEqual(await readStreamBytes(stream), Uint8Array.of(1, 2, 3));
    assert.deepEqual(calls, [
        { command: 'open_chat_backup_download', args: { name: 'chat_alice.jsonl' } },
        ...Array.from({ length: 3 }, () => ({ command: 'read_chat_backup_download', args: { rid: 8 } })),
        { command: 'plugin:resources|close', args: { rid: 8 } },
    ]);
});
