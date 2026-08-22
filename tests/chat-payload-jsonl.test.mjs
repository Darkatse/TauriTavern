import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}


test('jsonl: payloadToJsonl rejects non-object entries', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/jsonl.js'));
    const { payloadToJsonl } = mod;

    assert.throws(() => payloadToJsonl([null]), /must be an object/i);
    assert.throws(() => payloadToJsonl([1]), /must be an object/i);
});



test('jsonl: jsonlToPayload surfaces line numbers on invalid JSON', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/jsonl.js'));
    const { jsonlToPayload } = mod;

    assert.throws(() => jsonlToPayload('{"a":1}\n{bad}\n'), /Invalid JSONL at line 2/);
});

test('jsonl: payloadToJsonlByteChunks round-trips and respects maxChunkBytes', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/jsonl.js'));
    const { payloadToJsonl, payloadToJsonlByteChunks } = mod;

    const payload = [{ a: 1 }, { b: 2 }, { c: 3 }];
    const maxChunkBytes = 16;
    const chunks = Array.from(payloadToJsonlByteChunks(payload, { maxChunkBytes }));

    assert.ok(chunks.length > 1);
    for (const chunk of chunks) {
        assert.ok(chunk.byteLength <= maxChunkBytes);
    }

    const combined = Buffer.concat(chunks.map((chunk) => Buffer.from(chunk)));
    assert.equal(combined.toString('utf8'), payloadToJsonl(payload));
});




test('jsonl: jsonlStreamToPayload parses chunked stream input', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/jsonl.js'));
    const { jsonlStreamToPayload } = mod;

    const encoder = new TextEncoder();
    const stream = new ReadableStream({
        start(controller) {
            controller.enqueue(encoder.encode('{"a":1}\n{"b":'));
            controller.enqueue(encoder.encode('2}\n{"c":3}\n'));
            controller.close();
        },
    });

    const payload = await jsonlStreamToPayload(stream);
    assert.deepEqual(payload, [{ a: 1 }, { b: 2 }, { c: 3 }]);
});


test('jsonl: visitJsonlStream scans each chunk once for a large unterminated record', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/jsonl.js'));
    const { visitJsonlStream } = mod;

    const expected = { mes: 'x'.repeat(256 * 1024) };
    const bytes = new TextEncoder().encode(JSON.stringify(expected));
    let offset = 0;
    const stream = new ReadableStream({
        pull(controller) {
            if (offset >= bytes.length) {
                controller.close();
                return;
            }

            controller.enqueue(bytes.slice(offset, offset + 1024));
            offset += 1024;
        },
    });

    const originalIndexOf = String.prototype.indexOf;
    let scannedCharacters = 0;
    String.prototype.indexOf = function (searchString, position = 0) {
        if (searchString === '\n') {
            scannedCharacters += Math.max(0, this.length - position);
        }
        return originalIndexOf.call(this, searchString, position);
    };

    const visited = [];
    try {
        await visitJsonlStream(stream, (entry) => visited.push(entry));
    } finally {
        String.prototype.indexOf = originalIndexOf;
    }

    assert.deepEqual(visited, [expected]);
    assert.ok(scannedCharacters <= bytes.length * 2, `scanned ${scannedCharacters} characters`);
});

test('jsonl: visitJsonlStream fails on an invalid unterminated final line', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/jsonl.js'));
    const { visitJsonlStream } = mod;

    const stream = new ReadableStream({
        start(controller) {
            controller.enqueue(new TextEncoder().encode('{"a":1}\n{bad}'));
            controller.close();
        },
    });

    await assert.rejects(() => visitJsonlStream(stream, () => {}), /Invalid JSONL at line 2/);
});

test('jsonl: jsonlStreamToPayload cancels reader on parse error during streaming', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/jsonl.js'));
    const { jsonlStreamToPayload } = mod;

    const encoder = new TextEncoder();
    let pushed = false;
    let canceled = false;

    const stream = new ReadableStream({
        pull(controller) {
            if (pushed) {
                return;
            }

            pushed = true;
            controller.enqueue(encoder.encode('{"a":1}\n{bad}\n{"c":3}\n'));
        },
        cancel() {
            canceled = true;
        },
    });

    await assert.rejects(() => jsonlStreamToPayload(stream), /Invalid JSONL at line 2/);
    assert.equal(canceled, true);
});
