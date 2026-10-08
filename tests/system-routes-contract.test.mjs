import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerSystemRoutes } from '../src/tauri/main/routes/system-routes.js';

test('the host claims unimplemented /api endpoints for every method', () => {
    const router = createRouteRegistry();
    registerSystemRoutes(router, {}, { jsonResponse });

    for (const method of ['GET', 'POST', 'PUT', 'DELETE']) {
        assert.equal(router.canHandle(method, '/api/moving-ui/save'), true);
    }
    assert.equal(router.canHandle('GET', '/scripts/script.js'), false);
});
