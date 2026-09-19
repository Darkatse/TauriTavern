import assert from 'node:assert/strict';
import test from 'node:test';

import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerTtsRoutes } from '../src/tauri/main/routes/tts-routes.js';

function encodeBytes(bytes) {
    return Buffer.from(Uint8Array.from(bytes)).toString('base64');
}

function encodeText(text) {
    return Buffer.from(String(text), 'utf8').toString('base64');
}


test('grok tts route delegates generation to backend command', async () => {
    const router = createRouteRegistry();
    const safeInvokeCalls = [];
    const context = {
        safeInvoke: async (command, args) => {
            safeInvokeCalls.push({ command, args });
            return {
                status: 200,
                contentType: 'audio/mpeg',
                bodyBase64: encodeBytes([1, 2, 3]),
            };
        },
    };

    registerTtsRoutes(router, context);

    const body = {
        text: 'Hello world',
        voiceId: 'EVE',
        language: 'en',
        outputFormat: {
            codec: 'mp3',
            sampleRate: 44100,
            bitRate: 192000,
        },
    };
    const response = await router.handle({
        method: 'POST',
        path: '/api/tts/grok/generate',
        body,
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(response.headers.get('content-type'), 'audio/mpeg');
    assert.deepEqual(Array.from(new Uint8Array(await response.arrayBuffer())), [1, 2, 3]);
    assert.deepEqual(safeInvokeCalls, [
        {
            command: 'tts_handle',
            args: {
                path: 'grok/generate',
                body,
            },
        },
    ]);
});




test('tts route preserves backend validation response bodies', async () => {
    const router = createRouteRegistry();
    const message = 'No text provided';
    const context = {
        safeInvoke: async () => ({
            status: 400,
            contentType: 'text/plain; charset=utf-8',
            bodyBase64: encodeText(message),
            statusText: message,
        }),
    };

    registerTtsRoutes(router, context);

    const response = await router.handle({
        method: 'POST',
        path: '/api/azure/generate',
        body: {},
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.equal(response.statusText, message);
    assert.equal(await response.text(), message);
});

test('minimax tts route exposes backend errors as json without invalid statusText', async () => {
    const router = createRouteRegistry();
    const message = 'API Error: 音色不存在';
    const context = {
        safeInvoke: async () => ({
            status: 502,
            contentType: 'application/json; charset=utf-8',
            bodyBase64: encodeText(JSON.stringify({ error: message })),
            statusText: message,
        }),
    };

    registerTtsRoutes(router, context);

    const response = await router.handle({
        method: 'POST',
        path: '/api/minimax/generate-voice',
        body: {
            text: 'hello',
            voiceId: 'Chinese (Mandarin)_Unrestrained_Young_Man',
        },
    });

    assert.ok(response);
    assert.equal(response.status, 502);
    assert.equal(response.statusText, '');
    assert.deepEqual(await response.json(), { error: message });
});
