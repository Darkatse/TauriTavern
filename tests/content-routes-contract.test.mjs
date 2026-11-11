import assert from 'node:assert/strict';
import test from 'node:test';

import { getSillyTavernLocale } from '../src/tauri/main/adapters/st/sillytavern-i18n.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerContentRoutes } from '../src/tauri/main/routes/content-routes.js';

test('both content import routes return the host-staged content as SillyTavern does and release it', async () => {
    for (const { path, url, staged, body, contentType, disposition } of [
        {
            path: '/api/content/importURL',
            url: 'https://files.catbox.moe/0f3a.json',
            staged: { path: '/cache/content/0f3a.json', name: '角色 卡.json', kind: 'character' },
            body: '{"spec":"chara_card_v2"}',
            contentType: 'application/json',
            disposition: 'attachment; filename="%E8%A7%92%E8%89%B2%20%E5%8D%A1.json"',
        },
        {
            path: '/api/content/importUUID',
            url: 'lorebooks/bartleby/example-lorebook',
            staged: { path: '/cache/content/9b1c.json', name: 'example-lorebook.json', kind: 'lorebook' },
            body: '{"entries":{}}',
            contentType: 'application/json',
            disposition: 'attachment; filename="example-lorebook.json"',
        },
    ]) {
        const calls = [];
        const discarded = [];
        const router = createRouteRegistry();
        registerContentRoutes(router, {
            safeInvoke: async (command, args) => {
                calls.push({ command, args });
                return staged;
            },
            adoptStagedFile: filePath => ({ filePath, cleanup: async () => { discarded.push(filePath); } }),
            createReadableFileStream: async () => new Blob([body]).stream(),
        });

        const response = await router.handle({
            method: 'POST',
            path,
            url: new URL(`http://localhost${path}`),
            body: { url },
        });

        assert.equal(response.status, 200, path);
        assert.equal(response.headers.get('Content-Type'), contentType, path);
        assert.equal(response.headers.get('Content-Disposition'), disposition, path);
        assert.equal(response.headers.get('X-Custom-Content-Type'), staged.kind, path);
        assert.equal(await response.text(), body, path);
        assert.deepEqual(calls, [{
            command: 'stage_content_from_url',
            args: { url, locale: getSillyTavernLocale() },
        }], path);
        assert.deepEqual(discarded, [staged.path], path);
    }
});
