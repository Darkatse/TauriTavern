import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerProviderRoutes } from '../src/tauri/main/routes/provider-routes.js';
import { registerSettingsRoutes } from '../src/tauri/main/routes/settings-routes.js';
import { registerVectorRoutes } from '../src/tauri/main/routes/vector-routes.js';

const SECRET_BACKED_METADATA = [
    'get_openrouter_credits',
    'get_nanogpt_credits',
    'get_siliconflow_embedding_models',
    'get_workers_ai_embedding_models',
    'get_workers_ai_multimodal_models',
];

test('provider credential mutations invalidate only secret-backed metadata', async () => {
    const router = createRouteRegistry();
    const invalidations = [];
    registerSettingsRoutes(router, {
        safeInvoke: async () => 'secret-id',
        invalidateInvokeAll: command => invalidations.push(command),
    }, { jsonResponse });

    for (const request of [
        {
            path: '/api/secrets/write',
            body: { key: 'api_key_openrouter', value: 'new-key' },
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
        assert.equal((await router.handle({ method: 'POST', ...request })).status, 200);
        assert.deepEqual(invalidations, SECRET_BACKED_METADATA);
    }

    invalidations.length = 0;
    assert.equal((await router.handle({
        method: 'POST',
        path: '/api/secrets/write',
        body: { key: 'api_key_openai', value: 'new-key' },
    })).status, 200);
    assert.deepEqual(invalidations, []);
});

test('provider metadata routes preserve request identity and visible failures', async () => {
    const calls = [];
    const router = createRouteRegistry();
    registerProviderRoutes(router, {
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return [];
        },
    }, { jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/openai/workers-ai/models/embedding',
        body: { workers_ai_account_id: 'account-id' },
    });
    assert.equal(response.status, 200);
    assert.deepEqual(calls, [{
        command: 'get_workers_ai_embedding_models',
        args: { dto: { workers_ai_account_id: 'account-id' } },
    }]);

    const failingRouter = createRouteRegistry();
    registerProviderRoutes(failingRouter, {
        safeInvoke: async () => {
            throw new Error('provider unavailable');
        },
    }, { jsonResponse });
    await assert.rejects(
        failingRouter.handle({
            method: 'POST',
            path: '/api/openai/workers-ai/models/embedding',
            body: { workers_ai_account_id: 'account-id' },
        }),
        /provider unavailable/,
    );
});

test('vector compatibility routes preserve success and validation responses', async () => {
    const router = createRouteRegistry();
    const calls = [];
    registerVectorRoutes(router, {
        safeInvoke: async (command, payload) => {
            calls.push({ command, payload });
            if (payload.path === 'query') {
                throw new Error('Bad request: searchText is required');
            }
            return { status: 200, kind: 'empty', body: null };
        },
    }, { jsonResponse });

    const inserted = await router.handle({
        method: 'POST',
        path: '/api/vector/insert',
        body: { collectionId: 'chat-1', items: [], source: 'transformers' },
    });
    assert.equal(inserted.status, 200);
    assert.deepEqual(calls[0], {
        command: 'vector_handle',
        payload: {
            path: 'insert',
            request: { collectionId: 'chat-1', items: [], source: 'transformers' },
        },
    });

    const invalid = await router.handle({
        method: 'POST',
        path: '/api/vector/query',
        body: { collectionId: 'chat-1' },
    });
    assert.equal(invalid.status, 400);
    assert.deepEqual(await invalid.json(), {
        error: true,
        message: 'Bad request: searchText is required',
    });

    const unknown = await router.handle({
        method: 'POST',
        path: '/api/vector/not-yet-known',
        body: {},
    });
    assert.equal(unknown.status, 404);
    assert.deepEqual(await unknown.json(), {
        error: 'Unsupported vector endpoint: not-yet-known',
    });
});

test('vector routes classify operation-wide recoverable failures', async () => {
    for (const [message, status, cause] of [
        ['Unauthorized: bad key', 401, 'embedding_auth_failed'],
        ['Conflict: stale dimensions', 409, 'vector_index_conflict'],
        ['Too many requests: retry later', 429, 'embedding_rate_limited'],
        ['Internal server error: provider offline', 500, 'embedding_unavailable'],
    ]) {
        const router = createRouteRegistry();
        registerVectorRoutes(router, {
            safeInvoke: async () => {
                throw new Error(message);
            },
        }, { jsonResponse });

        const response = await router.handle({
            method: 'POST',
            path: '/api/vector/insert',
            body: {},
        });
        assert.equal(response.status, status);
        assert.equal((await response.json()).cause, cause);
    }
});
