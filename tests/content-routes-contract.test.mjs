import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerContentRoutes } from '../src/tauri/main/routes/content-routes.js';

function createContentRouter(context) {
    const router = createRouteRegistry();
    registerContentRoutes(router, context, { jsonResponse });
    return router;
}

test('/api/content/importURL allows configured import hosts without weakening remote HTTPS', async () => {
    const calls = [];
    const router = createContentRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { data: [137, 80, 78, 71], mimeType: 'image/png', fileName: 'Alice.png' };
        },
    });

    const allowedUrls = [
        'https://botbooru.com/post/123',
        'http://localhost:8000/card.png',
        'https://raw.githubusercontent.com/owner/repository/main/card.png',
    ];

    for (const importUrl of allowedUrls) {
        const response = await router.handle({
            method: 'POST',
            path: '/api/content/importURL',
            url: new URL('http://localhost/api/content/importURL'),
            body: { url: importUrl },
        });

        assert.equal(response.status, 200);
        assert.equal(response.headers.get('Content-Type'), 'image/png');
    }

    for (const importUrl of [
        'https://botbooru.com.evil.example/download/png/123',
        'http://raw.githubusercontent.com/owner/repository/main/card.png',
    ]) {
        const response = await router.handle({
            method: 'POST',
            path: '/api/content/importURL',
            url: new URL('http://localhost/api/content/importURL'),
            body: { url: importUrl },
        });

        assert.equal(response.status, 403);
    }

    assert.deepEqual(calls, allowedUrls.map((url) => ({
        command: 'download_external_import_url',
        args: { url },
    })));
});
