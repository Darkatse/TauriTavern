import assert from 'node:assert/strict';
import test from 'node:test';

import { textResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerTranslateRoutes } from '../src/tauri/main/routes/translate-routes.js';

test('translate routes forward provider to backend', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return 'Hello';
        },
    };

    registerTranslateRoutes(router, context, { textResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/translate/google',
        body: { text: '你好', lang: 'en' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(await response.text(), 'Hello');
    assert.deepEqual(calls, [
        {
            command: 'translate_text',
            args: {
                provider: 'google',
                body: { text: '你好', lang: 'en' },
            },
        },
    ]);
});

test('translate routes surface command errors via statusText', async () => {
    const router = createRouteRegistry();
    const context = {
        safeInvoke: async () => {
            throw new Error('Bad request: No DeepL API key');
        },
    };

    registerTranslateRoutes(router, context, { textResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/translate/deepl',
        body: { text: 'Hello', lang: 'ZH', endpoint: 'free' },
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.equal(response.statusText, 'Bad request: No DeepL API key');
});

