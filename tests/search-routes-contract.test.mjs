import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse, textResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerSearchRoutes } from '../src/tauri/main/routes/search-routes.js';

test('SearXNG search preserves the compatibility request and HTML response', async () => {
    const calls = [];
    const router = createRouteRegistry();
    registerSearchRoutes(router, {
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return '<article class="result">Tauri</article>';
        },
    }, { jsonResponse, textResponse });

    const body = {
        baseUrl: 'http://localhost:8888',
        query: 'Tauri',
        preferences: 'lang=en',
        categories: 'it',
    };
    const response = await router.handle({
        method: 'POST',
        path: '/api/search/searxng',
        body,
    });

    assert.equal(response.status, 200);
    assert.equal(response.headers.get('content-type'), 'text/html; charset=utf-8');
    assert.equal(await response.text(), '<article class="result">Tauri</article>');
    assert.equal(calls[0].command, 'search_searxng');
    assert.deepEqual(calls[0].args.dto, body);
    assert.equal(typeof calls[0].args.locale, 'string');
});

test('unsupported search routes fail explicitly', async () => {
    const router = createRouteRegistry();
    registerSearchRoutes(router, {
        safeInvoke: async () => assert.fail('unsupported route must not invoke Rust'),
    }, { jsonResponse, textResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/search/tavily',
        body: { query: 'Tauri' },
    });

    assert.equal(response.status, 404);
    assert.deepEqual(await response.json(), {
        error: 'Unsupported endpoint: /api/search/tavily',
    });
});
