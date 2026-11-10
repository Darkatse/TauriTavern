import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse, textResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerAssetsRoutes } from '../src/tauri/main/routes/assets-routes.js';

function createAssetsRouter(context) {
    const router = createRouteRegistry();
    registerAssetsRoutes(router, context, { jsonResponse, textResponse });
    return router;
}

test('/api/assets/get forwards to the native asset library command', async () => {
    const calls = [];
    const router = createAssetsRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { bgm: ['assets/bgm/theme.mp3'], vrm: { model: [], animation: [] } };
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/assets/get',
        url: new URL('http://localhost/api/assets/get'),
        body: {},
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
        bgm: ['assets/bgm/theme.mp3'],
        vrm: { model: [], animation: [] },
    });
    assert.deepEqual(calls, [{ command: 'get_assets_library', args: undefined }]);
});

test('/api/assets/download returns a character card through the host card download', async () => {
    const calls = [];
    const discarded = [];
    const router = createAssetsRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { path: '/cache/character-card/0f3a.png', name: 'card.png' };
        },
        adoptStagedFile: filePath => ({ filePath, cleanup: async () => { discarded.push(filePath); } }),
        createReadableFileStream: async () => new Blob([new Uint8Array([137, 80, 78, 71])]).stream(),
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/assets/download',
        url: new URL('http://localhost/api/assets/download'),
        body: {
            url: 'https://raw.githubusercontent.com/SillyTavern/SillyTavern-Content/main/card.png',
            category: 'character',
            filename: 'Seraphina.png',
        },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(response.headers.get('Content-Type'), 'image/png');
    assert.deepEqual([...new Uint8Array(await response.arrayBuffer())], [137, 80, 78, 71]);
    assert.deepEqual(calls, [{
        command: 'stage_character_card_from_url',
        args: { url: 'https://raw.githubusercontent.com/SillyTavern/SillyTavern-Content/main/card.png' },
    }]);
    assert.deepEqual(discarded, ['/cache/character-card/0f3a.png']);
});

test('/api/assets/download returns upstream-style OK for stored assets', async () => {
    const calls = [];
    const router = createAssetsRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/assets/download',
        url: new URL('http://localhost/api/assets/download'),
        body: {
            url: 'https://files.catbox.moe/theme.mp3',
            category: 'bgm',
            filename: 'theme.mp3',
        },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(await response.text(), 'OK');
    assert.deepEqual(calls, [{
        command: 'download_asset',
        args: {
            url: 'https://files.catbox.moe/theme.mp3',
            category: 'bgm',
            filename: 'theme.mp3',
        },
    }]);
});

test('/api/assets/delete forwards category and filename to native command', async () => {
    const calls = [];
    const router = createAssetsRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/assets/delete',
        url: new URL('http://localhost/api/assets/delete'),
        body: { category: 'ambient', filename: 'rain.ogg' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(await response.text(), 'OK');
    assert.deepEqual(calls, [{
        command: 'delete_asset',
        args: { category: 'ambient', filename: 'rain.ogg' },
    }]);
});

test('/api/assets/character reads name and category from query params', async () => {
    const calls = [];
    const router = createAssetsRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return ['/characters/Alice/bgm/theme.mp3'];
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/assets/character',
        url: new URL('http://localhost/api/assets/character?name=Alice&category=bgm'),
        body: {},
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), ['/characters/Alice/bgm/theme.mp3']);
    assert.deepEqual(calls, [{
        command: 'get_character_assets',
        args: { name: 'Alice', category: 'bgm' },
    }]);
});
