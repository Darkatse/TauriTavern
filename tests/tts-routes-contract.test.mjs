import assert from 'node:assert/strict';
import test from 'node:test';

import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerTtsRoutes } from '../src/tauri/main/routes/tts-routes.js';

test('grok tts route proxies audio generation with xAI request contract', async (t) => {
    const router = createRouteRegistry();
    const safeInvokeCalls = [];
    let upstreamCall = null;
    const context = {
        safeInvoke: async (command, args) => {
            safeInvokeCalls.push({ command, args });
            return { value: 'xai-secret' };
        },
    };

    const originalFetch = globalThis.fetch;
    t.after(() => {
        globalThis.fetch = originalFetch;
    });
    globalThis.fetch = async (url, init) => {
        upstreamCall = { url, init };
        return new Response(Uint8Array.from([1, 2, 3]), {
            status: 200,
            headers: {
                'Content-Type': 'audio/mpeg',
            },
        });
    };

    registerTtsRoutes(router, context);

    const response = await router.handle({
        method: 'POST',
        path: '/api/tts/grok/generate',
        body: {
            text: 'Hello world',
            voiceId: 'EVE',
            language: 'en',
            outputFormat: {
                codec: 'mp3',
                sampleRate: 44100,
                bitRate: 192000,
            },
        },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(response.headers.get('content-type'), 'audio/mpeg');
    assert.deepEqual(Array.from(new Uint8Array(await response.arrayBuffer())), [1, 2, 3]);
    assert.deepEqual(safeInvokeCalls, [
        {
            command: 'find_secret',
            args: {
                dto: {
                    key: 'api_key_xai',
                },
            },
        },
    ]);
    assert.equal(upstreamCall?.url, 'https://api.x.ai/v1/tts');
    assert.equal(upstreamCall?.init?.headers?.Authorization, 'Bearer xai-secret');
    assert.deepEqual(JSON.parse(upstreamCall?.init?.body), {
        text: 'Hello world',
        voice_id: 'eve',
        language: 'en',
        output_format: {
            codec: 'mp3',
            sample_rate: 44100,
            bit_rate: 192000,
        },
    });
});

test('grok voice list route proxies upstream voices', async (t) => {
    const router = createRouteRegistry();
    const context = {
        safeInvoke: async () => ({ value: 'xai-secret' }),
    };

    const originalFetch = globalThis.fetch;
    t.after(() => {
        globalThis.fetch = originalFetch;
    });
    globalThis.fetch = async (url) => {
        assert.equal(url, 'https://api.x.ai/v1/tts/voices');
        return Response.json({
            voices: [
                { voice_id: 'eve', name: 'Eve', language: 'multilingual' },
                { voice_id: 'una', name: 'Una', language: 'multilingual' },
            ],
        });
    };

    registerTtsRoutes(router, context);

    const response = await router.handle({
        method: 'POST',
        path: '/api/tts/grok/voices',
        body: {},
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
        voices: [
            { voice_id: 'eve', name: 'Eve', language: 'multilingual' },
            { voice_id: 'una', name: 'Una', language: 'multilingual' },
        ],
    });
});

test('grok tts route surfaces upstream error text via statusText', async (t) => {
    const router = createRouteRegistry();
    const context = {
        safeInvoke: async () => ({ value: 'xai-secret' }),
    };

    const originalFetch = globalThis.fetch;
    t.after(() => {
        globalThis.fetch = originalFetch;
    });
    globalThis.fetch = async () => new Response(JSON.stringify({
        error: {
            message: 'Rate limited',
        },
    }), {
        status: 429,
        headers: {
            'Content-Type': 'application/json',
        },
    });

    registerTtsRoutes(router, context);

    const response = await router.handle({
        method: 'POST',
        path: '/api/tts/grok/generate',
        body: {
            text: 'Hello world',
            voiceId: 'una',
        },
    });

    assert.ok(response);
    assert.equal(response.status, 429);
    assert.equal(response.statusText, 'Rate limited');
    assert.equal(await response.text(), 'Rate limited');
});

test('mimo tts route follows MiMo chat-completions audio contract', async (t) => {
    const router = createRouteRegistry();
    const safeInvokeCalls = [];
    let upstreamCall = null;
    const context = {
        safeInvoke: async (command, args) => {
            safeInvokeCalls.push({ command, args });
            return { value: 'mimo-secret' };
        },
    };

    const originalFetch = globalThis.fetch;
    t.after(() => {
        globalThis.fetch = originalFetch;
    });
    globalThis.fetch = async (url, init) => {
        upstreamCall = { url, init };
        return Response.json({
            choices: [
                {
                    message: {
                        audio: {
                            data: Buffer.from(Uint8Array.from([4, 5, 6])).toString('base64'),
                        },
                    },
                },
            ],
        });
    };

    registerTtsRoutes(router, context);

    const response = await router.handle({
        method: 'POST',
        path: '/api/tts/mimo/generate',
        body: {
            text: '你好，世界',
            voiceId: '冰糖',
            model: 'mimo-v2.5-tts',
            format: 'mp3',
            instructions: '活泼一点，语速稍快。',
        },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(response.headers.get('content-type'), 'audio/mpeg');
    assert.deepEqual(Array.from(new Uint8Array(await response.arrayBuffer())), [4, 5, 6]);
    assert.deepEqual(safeInvokeCalls, [
        {
            command: 'find_secret',
            args: {
                dto: {
                    key: 'api_key_mimo',
                },
            },
        },
    ]);
    assert.equal(upstreamCall?.url, 'https://api.xiaomimimo.com/v1/chat/completions');
    assert.equal(upstreamCall?.init?.headers['api-key'], 'mimo-secret');
    assert.deepEqual(JSON.parse(upstreamCall?.init?.body), {
        model: 'mimo-v2.5-tts',
        messages: [
            {
                role: 'user',
                content: '活泼一点，语速稍快。',
            },
            {
                role: 'assistant',
                content: '你好，世界',
            },
        ],
        audio: {
            format: 'mp3',
            voice: '冰糖',
        },
    });
});

test('mimo tts route rejects unsupported models before calling upstream', async (t) => {
    const router = createRouteRegistry();
    const context = {
        safeInvoke: async () => ({ value: 'mimo-secret' }),
    };

    const originalFetch = globalThis.fetch;
    t.after(() => {
        globalThis.fetch = originalFetch;
    });
    let fetchCalled = false;
    globalThis.fetch = async () => {
        fetchCalled = true;
        throw new Error('should not reach upstream');
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
    assert.equal(response.statusText, 'Unsupported MiMo model: mimo-v3-tts');
    assert.equal(await response.text(), 'Unsupported MiMo model: mimo-v3-tts');
    assert.equal(fetchCalled, false);
});
