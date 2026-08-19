import assert from 'node:assert/strict';
import test from 'node:test';

import { createReadableFileStreamService } from '../src/tauri/main/services/files/readable-file-stream-service.js';

function createFsReadResponse(payload) {
    const trailer = new Uint8Array(8);
    let length = payload.byteLength;
    for (let index = trailer.length - 1; index >= 0; index -= 1) {
        trailer[index] = length & 0xff;
        length = Math.floor(length / 0x100);
    }

    const response = new Uint8Array(payload.byteLength + trailer.byteLength);
    response.set(payload, 0);
    response.set(trailer, payload.byteLength);
    return response;
}

async function readStreamBytes(stream) {
    const reader = stream.getReader();
    const chunks = [];

    while (true) {
        const { done, value } = await reader.read();
        if (done) {
            break;
        }
        chunks.push(...value);
    }

    return chunks;
}

test('readable file stream service reads Tauri fs chunks and closes the resource', async () => {
    const calls = [];
    const readResponses = [
        createFsReadResponse(Uint8Array.from([0x50, 0x4b])),
        createFsReadResponse(Uint8Array.from([0x03, 0x04])),
        createFsReadResponse(new Uint8Array(0)),
    ];
    const service = createReadableFileStreamService({
        invoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'plugin:fs|open') {
                return 7;
            }
            if (command === 'plugin:fs|read') {
                return readResponses.shift();
            }
            if (command === 'plugin:resources|close') {
                return null;
            }
            throw new Error(`Unexpected command: ${command}`);
        },
    });

    const bytes = await readStreamBytes(service.createReadableFileStream('/tmp/archive.zip'));

    assert.deepEqual(bytes, [0x50, 0x4b, 0x03, 0x04]);
    assert.deepEqual(calls, [
        {
            command: 'plugin:fs|open',
            args: {
                path: '/tmp/archive.zip',
                options: { read: true },
            },
        },
        {
            command: 'plugin:fs|read',
            args: {
                rid: 7,
                len: 512 * 1024,
            },
        },
        {
            command: 'plugin:fs|read',
            args: {
                rid: 7,
                len: 512 * 1024,
            },
        },
        {
            command: 'plugin:fs|read',
            args: {
                rid: 7,
                len: 512 * 1024,
            },
        },
        {
            command: 'plugin:resources|close',
            args: {
                rid: 7,
            },
        },
    ]);
});

test('readable file stream service reads chat backup chunks and closes the resource', async () => {
    const calls = [];
    const readResponses = [
        Uint8Array.from([1, 2]),
        Uint8Array.from([3]),
        new Uint8Array(0),
    ];
    const service = createReadableFileStreamService({
        invoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'open_chat_backup_download') {
                return 8;
            }
            if (command === 'read_chat_backup_download') {
                return readResponses.shift();
            }
            if (command === 'plugin:resources|close') {
                return null;
            }
            throw new Error(`Unexpected command: ${command}`);
        },
    });

    const stream = await service.createChatBackupDownloadStream('chat_alice.jsonl');
    assert.deepEqual(await readStreamBytes(stream), [1, 2, 3]);
    assert.deepEqual(calls, [
        {
            command: 'open_chat_backup_download',
            args: { name: 'chat_alice.jsonl' },
        },
        ...Array.from({ length: 3 }, () => ({
            command: 'read_chat_backup_download',
            args: { rid: 8 },
        })),
        {
            command: 'plugin:resources|close',
            args: { rid: 8 },
        },
    ]);
});

test('readable resource stream closes on read failure', async () => {
    const calls = [];
    const service = createReadableFileStreamService({
        invoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'plugin:fs|open') {
                return 9;
            }
            if (command === 'plugin:fs|read') {
                return { bytes: [] };
            }
            if (command === 'plugin:resources|close') {
                return null;
            }
            throw new Error(`Unexpected command: ${command}`);
        },
    });

    const reader = service.createReadableFileStream('/tmp/archive.zip').getReader();

    await assert.rejects(() => reader.read(), /Unexpected resource read response/);
    assert.deepEqual(calls.at(-1), {
        command: 'plugin:resources|close',
        args: {
            rid: 9,
        },
    });
});
