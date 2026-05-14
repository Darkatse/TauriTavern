import assert from 'node:assert/strict';
import test from 'node:test';

import { textResponse, jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerCharacterRoutes } from '../src/tauri/main/routes/character-routes.js';
import { createCharacterService } from '../src/tauri/main/services/characters/character-service.js';
import { createCharacterFormService } from '../src/tauri/main/services/characters/character-form-service.js';

test('/api/characters/edit-avatar delegates multipart avatar replacement only', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        editCharacterAvatarFromForm: async (formData, url) => {
            calls.push({ formData, url });
        },
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const body = new FormData();
    body.set('avatar_url', 'Alice.png');
    body.set('avatar', new Blob(['avatar-bytes'], { type: 'image/png' }), 'avatar.png');
    const url = new URL('http://localhost/api/characters/edit-avatar?crop=%7B%7D');

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/edit-avatar',
        url,
        body,
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(await response.text(), 'OK');
    assert.equal(calls.length, 1);
    assert.equal(calls[0].formData, body);
    assert.equal(calls[0].url, url);
});

test('/api/characters/edit-avatar rejects non-multipart payloads', async () => {
    const router = createRouteRegistry();
    const context = {
        editCharacterAvatarFromForm: async () => {
            throw new Error('should not be called');
        },
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/edit-avatar',
        url: new URL('http://localhost/api/characters/edit-avatar'),
        body: { avatar_url: 'Alice.png' },
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.deepEqual(await response.json(), { error: 'Expected multipart form data' });
});

test('character form service maps edit-avatar to update_avatar without full character rewrite', async () => {
    const invokes = [];
    const invalidations = [];
    const cleanups = [];
    const service = createCharacterFormService({
        safeInvoke: async (command, args) => {
            invokes.push({ command, args });
        },
        invalidateInvokeAll: (command) => {
            invalidations.push(command);
        },
        resolveCharacterId: async () => {
            throw new Error('resolveCharacterId should not be called');
        },
        resolveExistingCharacterId: async ({ avatar }) => {
            assert.equal(avatar, 'Alice.png');
            return 'Alice';
        },
        materializeUploadFile: async (file, options) => {
            assert.ok(file instanceof Blob);
            assert.deepEqual(options, { preferredName: 'avatar.png' });
            return {
                filePath: '/tmp/avatar.png',
                cleanup: async () => cleanups.push('avatar.png'),
            };
        },
    });

    const formData = new FormData();
    formData.set('avatar_url', 'Alice.png');
    formData.set('avatar', new Blob(['avatar-bytes'], { type: 'image/png' }), 'avatar.png');

    await service.editCharacterAvatarFromForm(
        formData,
        new URL('http://localhost/api/characters/edit-avatar?crop=%7B%22x%22%3A1%2C%22y%22%3A2%2C%22width%22%3A300%2C%22height%22%3A400%2C%22want_resize%22%3Atrue%7D'),
    );

    assert.deepEqual(invokes, [
        {
            command: 'update_avatar',
            args: {
                dto: {
                    name: 'Alice',
                    avatar_path: '/tmp/avatar.png',
                    crop: {
                        x: 1,
                        y: 2,
                        width: 300,
                        height: 400,
                        want_resize: true,
                    },
                },
            },
        },
    ]);
    assert.deepEqual(invalidations, ['read_thumbnail_asset']);
    assert.deepEqual(cleanups, ['avatar.png']);
});

test('character form service fails fast on invalid avatar_url', async () => {
    const service = createCharacterFormService({
        safeInvoke: async () => {
            throw new Error('should not be called');
        },
        invalidateInvokeAll: () => {
            throw new Error('should not be called');
        },
        resolveCharacterId: async () => {
            throw new Error('should not be called');
        },
        resolveExistingCharacterId: async () => {
            throw new Error('should not be called');
        },
        materializeUploadFile: async () => {
            throw new Error('should not be called');
        },
    });

    const formData = new FormData();
    formData.set('avatar_url', '../Alice.png');
    formData.set('avatar', new Blob(['avatar-bytes'], { type: 'image/png' }), 'avatar.png');

    await assert.rejects(
        service.editCharacterAvatarFromForm(
            formData,
            new URL('http://localhost/api/characters/edit-avatar'),
        ),
        /Bad request: invalid avatar_url/,
    );
});

test('character form service keeps missing edit-avatar target on upstream 400 contract', async () => {
    const service = createCharacterFormService({
        safeInvoke: async () => {
            throw new Error('should not be called');
        },
        invalidateInvokeAll: () => {
            throw new Error('should not be called');
        },
        resolveCharacterId: async ({ avatar }) => {
            throw new Error(`resolveCharacterId should not be called for ${avatar}`);
        },
        resolveExistingCharacterId: async ({ avatar }) => {
            assert.equal(avatar, 'Missing.png');
            return null;
        },
        materializeUploadFile: async () => {
            throw new Error('should not be called');
        },
    });

    const formData = new FormData();
    formData.set('avatar_url', 'Missing.png');
    formData.set('avatar', new Blob(['avatar-bytes'], { type: 'image/png' }), 'avatar.png');

    await assert.rejects(
        service.editCharacterAvatarFromForm(
            formData,
            new URL('http://localhost/api/characters/edit-avatar'),
        ),
        /Bad request: character file does not exist/,
    );
});

test('character service strict resolver does not synthesize missing avatar ids', async () => {
    const invokes = [];
    const service = createCharacterService({
        safeInvoke: async (command, args) => {
            invokes.push({ command, args });
            assert.equal(command, 'get_all_characters');
            assert.deepEqual(args, { shallow: true });
            return [];
        },
    });

    assert.equal(await service.resolveExistingCharacterId({ avatar: 'Missing.png' }), null);
    assert.equal(await service.resolveCharacterId({ avatar: 'Missing.png' }), 'Missing');
    assert.equal(invokes.length, 2);
});

test('character service strict resolver refreshes stale caches before missing verdicts', async () => {
    const responses = [
        [{ name: 'Alice', avatar: 'Alice.png' }],
        [],
    ];
    const service = createCharacterService({
        safeInvoke: async (command, args) => {
            assert.equal(command, 'get_all_characters');
            assert.deepEqual(args, { shallow: true });
            return responses.shift() || [];
        },
    });

    assert.equal(await service.resolveExistingCharacterId({ avatar: 'Alice.png' }), 'Alice');
    assert.equal(await service.resolveExistingCharacterId({ avatar: 'Missing.png' }), null);
    assert.equal(responses.length, 0);
});
