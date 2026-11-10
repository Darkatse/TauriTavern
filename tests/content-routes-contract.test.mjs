import assert from 'node:assert/strict';
import test from 'node:test';

import { getSillyTavernLocale } from '../src/tauri/main/adapters/st/sillytavern-i18n.js';
import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerContentRoutes } from '../src/tauri/main/routes/content-routes.js';

function createContentRouter(context) {
    const router = createRouteRegistry();
    registerContentRoutes(router, context, { jsonResponse });
    return router;
}

test('/api/content/importURL returns the host-staged card as SillyTavern does and releases it', async () => {
    const calls = [];
    const discarded = [];
    const router = createContentRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { path: '/cache/character-card/0f3a.json', name: '角色 卡.json' };
        },
        adoptStagedFile: filePath => ({ filePath, cleanup: async () => { discarded.push(filePath); } }),
        createReadableFileStream: async () => new Blob(['{"spec":"chara_card_v2"}']).stream(),
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/content/importURL',
        url: new URL('http://localhost/api/content/importURL'),
        body: { url: 'https://files.catbox.moe/0f3a.json' },
    });

    assert.equal(response.status, 200);
    assert.equal(response.headers.get('Content-Type'), 'application/json');
    assert.equal(
        response.headers.get('Content-Disposition'),
        'attachment; filename="%E8%A7%92%E8%89%B2%20%E5%8D%A1.json"',
    );
    assert.equal(response.headers.get('X-Custom-Content-Type'), 'character');
    assert.equal(await response.text(), '{"spec":"chara_card_v2"}');
    assert.deepEqual(calls, [{
        command: 'stage_character_card_from_url',
        args: { url: 'https://files.catbox.moe/0f3a.json', locale: getSillyTavernLocale() },
    }]);
    assert.deepEqual(discarded, ['/cache/character-card/0f3a.json']);
});
