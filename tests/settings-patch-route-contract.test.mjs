import test from 'node:test';
import assert from 'node:assert/strict';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { extractErrorText, resolveHostErrorResponse } from '../src/tauri/main/kernel/host-error-response.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerSettingsRoutes } from '../src/tauri/main/routes/settings-routes.js';

function createSettingsRouter(context) {
    const router = createRouteRegistry();
    registerSettingsRoutes(router, context, { jsonResponse });
    return router;
}

test('/api/settings/patch forwards the patch DTO to the native command', async () => {
    const calls = [];
    const patch = {
        hash_algorithm: 'tt-user-settings-stable-sha256-v1',
        base_hash: 'base',
        ops: [{ op: 'set', path: ['username'], value: 'Alice' }],
    };
    const router = createSettingsRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { result: 'ok', mode: 'patch' };
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/settings/patch',
        body: patch,
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { result: 'ok', mode: 'patch' });
    assert.deepEqual(calls, [{
        command: 'save_user_settings_patch',
        args: { patch },
    }]);
});

test('/api/settings/patch CAS conflicts map to HTTP 409 at the host boundary', async () => {
    const router = createSettingsRouter({
        safeInvoke: async () => {
            throw new Error('Conflict: stale settings revision');
        },
    });

    let thrown = null;
    try {
        await router.handle({
            method: 'POST',
            path: '/api/settings/patch',
            body: {},
        });
    } catch (error) {
        thrown = error;
    }

    assert.ok(thrown);
    assert.deepEqual(resolveHostErrorResponse(extractErrorText(thrown)), {
        status: 409,
        body: 'Conflict: stale settings revision',
    });
});
