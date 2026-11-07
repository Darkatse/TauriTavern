import assert from 'node:assert/strict';
import test from 'node:test';

import { createRouteRegistry } from '../src/tauri/main/router.js';
import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { registerWorldInfoRoutes } from '../src/tauri/main/routes/worldinfo-routes.js';

// Extension discovery needs the metadata from every persisted book, not names alone.
test('world info inventory preserves file identities and book-level extension metadata', async () => {
    const inventory = [
        { file_id: 'world-one', name: 'Display name', extensions: { fixture: { bookId: 'one' } } },
        { file_id: '普通世界书', name: '普通世界书', extensions: {} },
    ];
    const router = createRouteRegistry();
    registerWorldInfoRoutes(router, {
        async safeInvoke(command) {
            assert.equal(command, 'list_world_info');
            return inventory;
        },
    }, { jsonResponse });
    const response = await router.handle({ method: 'POST', path: '/api/worldinfo/list', body: {} });
    assert.ok(response, 'POST /api/worldinfo/list must be handled by the host');
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), inventory);
});
