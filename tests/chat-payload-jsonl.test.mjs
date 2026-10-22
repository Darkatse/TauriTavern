import test from 'node:test';
import assert from 'node:assert/strict';
import {
    jsonlToPayload, jsonlStreamToPayload, payloadToJsonl,
    visitJsonlStream,
} from '../src/scripts/tauri/chat/jsonl.js';

function byteStream(bytes, chunkSize = 1) {
    let offset = 0;
    return new ReadableStream({
        pull(controller) {
            if (offset === bytes.length) return controller.close();
            controller.enqueue(bytes.subarray(offset, offset + chunkSize));
            offset = Math.min(bytes.length, offset + chunkSize);
        },
    });
}

test('jsonl: text and byte streams agree on the document preamble and record shape', async () => {
    const payload = [
        { chat_metadata: { integrity: ' \t\n' }, unknown: [true] },
        { mes: 'text\uFEFF�', chat_metadata: { integrity: 'ordinary message data' } },
    ];
    const canonical = payload.map(record => JSON.stringify(record)).join('\n');
    for (const prefix of ['\uFEFF', '\n\t \uFEFF \r\n\t']) {
        const text = prefix + canonical;
        assert.deepEqual(jsonlToPayload(text), payload);
        assert.deepEqual(await jsonlStreamToPayload(byteStream(new TextEncoder().encode(text))), payload);
        assert.equal(payloadToJsonl(jsonlToPayload(text)), canonical);
    }
    for (const text of ['', ' \n\uFEFF\n ']) {
        assert.deepEqual(jsonlToPayload(text), []);
        assert.deepEqual(await jsonlStreamToPayload(byteStream(new TextEncoder().encode(text))), []);
    }
    for (const text of [
        '\uFEFF\uFEFF{}', '\uFEFF\n\uFEFF\n{}', '{}\n\uFEFF{}',
        '\u0000{}', '{}\u00A0', '[]', '{}\nnull', '{bad}\n{}',
        '{"chat_metadata":{"integrity":null}}',
        '{"chat_metadata":{"integrity":""}}',
    ]) {
        assert.throws(() => jsonlToPayload(text), text);
        await assert.rejects(jsonlStreamToPayload(byteStream(new TextEncoder().encode(text))), text);
    }
    const openFields = [{ chat_metadata: 42, user_name: [], future: true }];
    assert.deepEqual(jsonlToPayload(payloadToJsonl(openFields)), openFields);
});

test('jsonl: byte streams reject malformed UTF-8 and truncated final characters', async () => {
    for (const bytes of [
        Buffer.concat([Buffer.from('{}\n{"x":"'), Buffer.from([0xFF]), Buffer.from('"}')]),
        Buffer.concat([Buffer.from('{}\n{"x":"'), Buffer.from([0xE4, 0xB8])]),
    ]) {
        await assert.rejects(jsonlStreamToPayload(byteStream(bytes)));
    }
});

test('jsonl: stream visitors validate unterminated final records', async () => {
    const payload = [{}, { mes: 'x'.repeat(256 * 1024) }];
    const bytes = new TextEncoder().encode(payload.map(record => JSON.stringify(record)).join('\n'));
    const visited = [];
    await visitJsonlStream(byteStream(bytes, 1024), entry => visited.push(entry));
    assert.deepEqual(visited, payload);
    await assert.rejects(visitJsonlStream(byteStream(new TextEncoder().encode('{}\n{bad}')), () => {}), /line 2/);
});

test('jsonl: parse failure cancels an unfinished stream', async () => {
    let pushed = false;
    let canceled = false;
    const stream = new ReadableStream({
        pull(controller) {
            if (pushed) return;
            pushed = true;
            controller.enqueue(new TextEncoder().encode('{"a":1}\n{bad}\n{"c":3}\n'));
        },
        cancel() { canceled = true; },
    });
    await assert.rejects(jsonlStreamToPayload(stream), /line 2/);
    assert.equal(canceled, true);
});
