import assert from 'node:assert/strict';
import test from 'node:test';

import { installExtensionStoreApi } from '../src/tauri/main/api/extension-store.js';

const target = { namespace: 'example', key: 'entry' };

function installStore(t, invoke) {
    const previousWindow = globalThis.window;
    globalThis.window = { __TAURITAVERN__: { api: {} } };
    t.after(() => { globalThis.window = previousWindow; });
    installExtensionStoreApi({ safeInvoke: invoke, invokeTransport: invoke });
    return window.__TAURITAVERN__.api.extension.store;
}

test('writes preserve call-time values and normalize Blob inputs', async t => {
    let gate = Promise.withResolvers();
    let received = [];
    const store = installStore(t, async (command, args, options) => {
        switch (command) {
            case 'begin_extension_store_commit':
                await gate.promise;
                return { sessionId: 'write', maxFrameBytes: 16 };
            case 'append_extension_store_commit_chunk':
                received.push(Buffer.from(args));
                return Number(options.headers.offset) + args.length;
            case 'finish_extension_store_commit':
                return;
            default:
                throw new Error(`Unexpected command: ${command}`);
        }
    });

    const value = { text: 'captured value' };
    const savingJson = store.setJson({ ...target, value });
    value.text = 'later edit';
    gate.resolve();
    await savingJson;
    assert.deepEqual(JSON.parse(Buffer.concat(received).toString()), { text: 'captured value' });

    received = [];
    gate = Promise.withResolvers();
    const backing = Uint8Array.of(9, 0, 255, 128, 1, 9);
    const savingBytes = store.setBlob({ ...target, data: backing.subarray(1, 5) });
    backing.fill(7);
    gate.resolve();
    await savingBytes;
    assert.deepEqual(Buffer.concat(received), Buffer.from([0, 255, 128, 1]));

    received = [];
    await store.setBlob({ ...target, data: 'AP+AAQ==' });
    assert.deepEqual(Buffer.concat(received), Buffer.from([0, 255, 128, 1]));
    await assert.rejects(store.setBlob({ ...target, data: 'YR==' }));

    received = [];
    await store.setBlob({ ...target, data: new Blob() });
    assert.equal(Buffer.concat(received).length, 0);
});

test('JSON lookup distinguishes missing from null and rejects malformed UTF-8', async t => {
    let chunks;
    const store = installStore(t, async (command, args) => {
        switch (command) {
            case 'open_extension_store_entry':
                if (args.key === 'missing') return null;
                chunks = args.key === 'null'
                    ? [Buffer.from('nu'), Buffer.from('ll'), new Uint8Array()]
                    : [Uint8Array.of(34, 255, 34), new Uint8Array()];
                return { readerId: 1, mimeType: 'application/json' };
            case 'read_bytes':
                return chunks.shift();
            case 'plugin:resources|close':
                return;
            default:
                throw new Error(`Unexpected command: ${command}`);
        }
    });

    assert.deepEqual(await store.tryGetJson({ ...target, key: 'missing' }), { found: false });
    assert.deepEqual(await store.tryGetJson({ ...target, key: 'null' }), { found: true, value: null });
    await assert.rejects(store.getJson({ ...target, key: 'invalid-utf8' }));
});
