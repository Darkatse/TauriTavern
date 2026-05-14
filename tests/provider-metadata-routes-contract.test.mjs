import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerSettingsRoutes } from '../src/tauri/main/routes/settings-routes.js';

const SECRET_BACKED_PROVIDER_METADATA_COMMANDS = [
    'get_openrouter_credits',
    'get_nanogpt_credits',
    'get_siliconflow_embedding_models',
    'get_workers_ai_embedding_models',
    'get_workers_ai_multimodal_models',
];

test('provider secret mutations invalidate secret-backed metadata caches', async () => {
    const router = createRouteRegistry();
    const invokes = [];
    const invalidations = [];
    const context = {
        safeInvoke: async (command, args) => {
            invokes.push({ command, args });
            return command === 'write_secret' ? 'secret-id' : undefined;
        },
        invalidateInvokeAll: (command) => {
            invalidations.push(command);
        },
    };

    registerSettingsRoutes(router, context, { jsonResponse });

    for (const request of [
        {
            path: '/api/secrets/write',
            body: { key: 'api_key_openrouter', value: 'new-key', label: 'OpenRouter' },
        },
        {
            path: '/api/secrets/delete',
            body: { key: 'api_key_nanogpt', id: 'secret-id' },
        },
        {
            path: '/api/secrets/rotate',
            body: { key: 'api_key_workers_ai', id: 'secret-id' },
        },
    ]) {
        invalidations.length = 0;
        const response = await router.handle({ method: 'POST', ...request });

        assert.ok(response);
        assert.equal(response.status, 200);
        assert.deepEqual(invalidations, SECRET_BACKED_PROVIDER_METADATA_COMMANDS);
    }

    assert.deepEqual(invokes.map(call => call.command), ['write_secret', 'delete_secret', 'rotate_secret']);
});

test('unrelated secret mutations leave provider metadata caches intact', async () => {
    const router = createRouteRegistry();
    const invalidations = [];
    const context = {
        safeInvoke: async () => 'secret-id',
        invalidateInvokeAll: (command) => {
            invalidations.push(command);
        },
    };

    registerSettingsRoutes(router, context, { jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/secrets/write',
        body: { key: 'api_key_openai', value: 'new-key' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(invalidations, []);
});
