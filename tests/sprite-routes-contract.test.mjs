import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse, textResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerSpriteRoutes } from '../src/tauri/main/routes/sprite-routes.js';

function createSpriteRouter(context) {
    const router = createRouteRegistry();
    registerSpriteRoutes(router, context, { jsonResponse, textResponse });
    return router;
}

test('/api/sprites/get forwards the sprite set and returns native sprites', async () => {
    const calls = [];
    const router = createSpriteRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return [{ label: 'joy', path: '/characters/Alice/joy.png?t=1' }];
        },
    });

    const response = await router.handle({
        method: 'GET',
        path: '/api/sprites/get',
        url: new URL('http://localhost/api/sprites/get?name=Alice%2Fformal'),
    });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), [{ label: 'joy', path: '/characters/Alice/joy.png?t=1' }]);
    assert.deepEqual(calls, [{ command: 'list_sprites', args: { dto: { name: 'Alice/formal' } } }]);
});

test('/api/sprites/upload stages, invokes, and cleans up a sprite', async () => {
    const calls = [];
    let cleanups = 0;
    const router = createSpriteRouter({
        materializeUploadFile: async (file, options) => {
            assert.equal(file.name, 'joy.webp');
            assert.deepEqual(options, { kind: 'sprite', preferredName: 'joy.webp' });
            return {
                filePath: '/tmp/joy.webp',
                cleanup: async () => { cleanups += 1; },
            };
        },
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
        },
    });
    const body = new FormData();
    body.append('name', 'Alice');
    body.append('label', 'joy');
    body.append('spriteName', 'joy-1');
    body.append('avatar', new File([new Uint8Array([1])], 'joy.webp', { type: 'image/webp' }));

    const response = await router.handle({ method: 'POST', path: '/api/sprites/upload', body });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { ok: true });
    assert.equal(cleanups, 1);
    assert.deepEqual(calls, [{
        command: 'upload_sprite',
        args: {
            dto: {
                name: 'Alice',
                sprite_name: 'joy-1',
                original_filename: 'joy.webp',
                file_path: '/tmp/joy.webp',
            },
        },
    }]);
});

test('/api/sprites/upload-zip returns the imported image count', async () => {
    const router = createSpriteRouter({
        materializeUploadFile: async (_file, options) => {
            assert.deepEqual(options, {
                kind: 'sprite-pack',
                preferredName: 'pack.zip',
                preferredExtension: 'zip',
            });
            return { filePath: '/tmp/pack.zip', cleanup: async () => {} };
        },
        safeInvoke: async (command, args) => {
            assert.equal(command, 'upload_sprite_pack');
            assert.deepEqual(args, { dto: { name: 'Alice/formal', file_path: '/tmp/pack.zip' } });
            return 3;
        },
    });
    const body = new FormData();
    body.append('name', 'Alice/formal');
    body.append('avatar', new File([new Uint8Array([1])], 'pack.zip', { type: 'application/zip' }));

    const response = await router.handle({ method: 'POST', path: '/api/sprites/upload-zip', body });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { ok: true, count: 3 });
});

test('/api/sprites/delete preserves upstream spriteName fallback and OK response', async () => {
    const calls = [];
    const router = createSpriteRouter({
        safeInvoke: async (command, args) => calls.push({ command, args }),
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/sprites/delete',
        body: { name: 'Alice', label: 'sad' },
    });

    assert.equal(response.status, 200);
    assert.equal(await response.text(), 'OK');
    assert.deepEqual(calls, [{
        command: 'delete_sprite',
        args: { dto: { name: 'Alice', sprite_name: 'sad' } },
    }]);
});

test('sprite upload cleanup still runs when the native command fails', async () => {
    let cleanups = 0;
    const router = createSpriteRouter({
        materializeUploadFile: async () => ({
            filePath: '/tmp/joy.png',
            cleanup: async () => { cleanups += 1; },
        }),
        safeInvoke: async () => { throw new Error('upload failed'); },
    });
    const body = new FormData();
    body.append('name', 'Alice');
    body.append('label', 'joy');
    body.append('avatar', new File([new Uint8Array([1])], 'joy.png', { type: 'image/png' }));

    await assert.rejects(
        router.handle({ method: 'POST', path: '/api/sprites/upload', body }),
        /upload failed/,
    );
    assert.equal(cleanups, 1);
});

test('sprite upload reports staging failures through the shared host error contract', async () => {
    const router = createSpriteRouter({
        materializeUploadFile: async () => ({ error: 'staging unavailable' }),
    });
    const body = new FormData();
    body.append('name', 'Alice');
    body.append('label', 'joy');
    body.append('avatar', new File([new Uint8Array([1])], 'joy.png', { type: 'image/png' }));

    await assert.rejects(
        router.handle({ method: 'POST', path: '/api/sprites/upload', body }),
        /Bad request: Unable to access uploaded file: staging unavailable/,
    );
});
