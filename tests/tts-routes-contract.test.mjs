import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
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

test('grok voice list route delegates to backend command', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const payload = {
        voices: [
            { voice_id: 'eve', name: 'Eve', language: 'multilingual' },
            { voice_id: 'ara', name: 'Ara', language: 'multilingual' },
        ],
    };
    const context = {
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return {
                status: 200,
                contentType: 'application/json',
                bodyBase64: encodeText(JSON.stringify(payload)),
            };
        },
    };

    registerTtsRoutes(router, context);

    const response = await router.handle({
        method: 'POST',
        path: '/api/tts/grok/voices',
        body: {},
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), payload);
    assert.deepEqual(calls, [
        {
            command: 'tts_handle',
            args: {
                path: 'grok/voices',
                body: {},
            },
        },
    ]);
});

test('tts route surfaces command errors via statusText', async () => {
    const router = createRouteRegistry();
    const context = {
        safeInvoke: async () => {
            throw new Error('Bad request: xAI API key is required');
        },
    };

    registerTtsRoutes(router, context);

    const response = await router.handle({
        method: 'POST',
        path: '/api/tts/grok/generate',
        body: {
            text: 'Hello world',
            voiceId: 'eve',
        },
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.equal(response.statusText, 'Bad request: xAI API key is required');
    assert.equal(await response.text(), 'Bad request: xAI API key is required');
});

test('mimo tts route delegates generation to backend command', async () => {
    const router = createRouteRegistry();
    const safeInvokeCalls = [];
    const context = {
        safeInvoke: async (command, args) => {
            safeInvokeCalls.push({ command, args });
            return {
                status: 200,
                contentType: 'audio/mpeg',
                bodyBase64: encodeBytes([4, 5, 6]),
            };
        },
    };

    registerTtsRoutes(router, context);

    const body = {
        text: '你好，世界',
        voiceId: '冰糖',
        model: 'mimo-v2.5-tts',
        format: 'mp3',
        instructions: '活泼一点，语速稍快。',
    };
    const response = await router.handle({
        method: 'POST',
        path: '/api/tts/mimo/generate',
        body,
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(response.headers.get('content-type'), 'audio/mpeg');
    assert.deepEqual(Array.from(new Uint8Array(await response.arrayBuffer())), [4, 5, 6]);
    assert.deepEqual(safeInvokeCalls, [
        {
            command: 'tts_handle',
            args: {
                path: 'mimo/generate',
                body,
            },
        },
    ]);
});

test('tts route preserves backend validation response bodies', async () => {
    const router = createRouteRegistry();
    const message = 'Unsupported MiMo model: mimo-v3-tts';
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
        path: '/api/tts/mimo/generate',
        body: {
            text: 'hello',
            model: 'mimo-v3-tts',
        },
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.equal(response.statusText, message);
    assert.equal(await response.text(), message);
});

test('grok provider voice list contract avoids silent fallback', async () => {
    const source = await readFile(new URL('../src/scripts/extensions/tts/grok.js', import.meta.url), 'utf8');

    assert.doesNotMatch(source, /voice_id:\s*'una'/);
    assert.doesNotMatch(source, /using fallback/i);
    assert.match(source, /Grok voice list response did not include any voices/);
});

test('tts host route stays a backend-command adapter', async () => {
    const source = await readFile(new URL('../src/tauri/main/routes/tts-routes.js', import.meta.url), 'utf8');

    assert.doesNotMatch(source, /api\.x\.ai/);
    assert.doesNotMatch(source, /xiaomimimo/);
    assert.doesNotMatch(source, /find_secret/);
    assert.doesNotMatch(source, /\bfetch\s*\(/);
    assert.match(source, /tts_handle/);
});
