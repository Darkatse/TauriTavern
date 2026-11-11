import assert from 'node:assert/strict';
import test from 'node:test';

import { textResponse } from '../src/tauri/main/http-utils.js';
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
    }, { textResponse });

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
