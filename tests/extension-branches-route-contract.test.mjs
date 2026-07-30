import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerExtensionRoutes } from '../src/tauri/main/routes/extensions-routes.js';

function createExtensionRouter(safeInvoke) {
    const router = createRouteRegistry();
    registerExtensionRoutes(router, { safeInvoke }, { jsonResponse });
    return router;
}

test('/api/extensions/branches preserves the upstream response shape', async () => {
    const calls = [];
    const router = createExtensionRouter(async (command, args) => {
        calls.push({ command, args });
        return [{
            name: 'feature/mobile',
            commit: '1234567',
            current: true,
            label: '',
        }];
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/extensions/branches',
        body: { extensionName: 'third-party/example', global: true },
    });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), [{
        name: 'feature/mobile',
        commit: '1234567',
        current: true,
        label: '',
    }]);
    assert.deepEqual(calls, [{
        command: 'get_extension_branches',
        args: { extensionName: 'third-party/example', global: true },
    }]);
});

test('/api/extensions/switch returns an empty 204 response', async () => {
    const calls = [];
    const router = createExtensionRouter(async (command, args) => {
        calls.push({ command, args });
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/extensions/switch',
        body: {
            extensionName: 'third-party/example',
            branch: 'feature/mobile',
            global: false,
        },
    });

    assert.equal(response.status, 204);
    assert.equal(await response.text(), '');
    assert.deepEqual(calls, [{
        command: 'switch_extension_branch',
        args: {
            extensionName: 'third-party/example',
            branch: 'feature/mobile',
            global: false,
        },
    }]);
});
