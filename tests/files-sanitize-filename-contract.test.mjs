import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse, textResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerResourceRoutes } from '../src/tauri/main/routes/resource-routes.js';

function createResourceRouter(context) {
    const router = createRouteRegistry();
    registerResourceRoutes(router, context, { jsonResponse, textResponse });
    return router;
}

test('/api/files/sanitize-filename delegates to Rust upstream-compatible filename contract', async () => {
    const upstreamResults = new Map([
        [' name ', ' name'],
        ['a:b*c?.json', 'abc.json'],
        ['a/b.json', 'ab.json'],
        ['CON.json', ''],
        ['中文/ 测试', '中文 测试'],
        ['a\u0000b', 'ab'],
    ]);
    const calls = [];
    const router = createResourceRouter({
        async safeInvoke(command, args) {
            calls.push({ command, args });
            assert.equal(command, 'sanitize_filename');
            assert.ok(upstreamResults.has(args.file_name));
            return upstreamResults.get(args.file_name);
        },
    });

    for (const [fileName, expected] of upstreamResults) {
        const response = await router.handle({
            method: 'POST',
            path: '/api/files/sanitize-filename',
            url: new URL('http://localhost/api/files/sanitize-filename'),
            body: { fileName },
        });

        assert.ok(response);
        assert.equal(response.status, 200);
        assert.deepEqual(await response.json(), { fileName: expected });
    }

    assert.deepEqual(calls.map(call => call.args.file_name), [...upstreamResults.keys()]);
});

test('/api/files/sanitize-filename rejects missing input before invoking Rust', async () => {
    const router = createResourceRouter({
        async safeInvoke() {
            throw new Error('safeInvoke should not be called');
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/files/sanitize-filename',
        url: new URL('http://localhost/api/files/sanitize-filename'),
        body: { fileName: '' },
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.equal(await response.text(), 'No fileName specified');
});
