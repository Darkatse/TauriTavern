import assert from 'node:assert/strict';
import test from 'node:test';

import { textResponse, jsonResponse } from '../src/tauri/main/http-utils.js';
import { resolveHostErrorResponse } from '../src/tauri/main/kernel/host-error-response.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerCharacterRoutes } from '../src/tauri/main/routes/character-routes.js';
import { createCharacterService } from '../src/tauri/main/services/characters/character-service.js';
import {
    CHARACTER_CREATE_WARNINGS,
    createCharacterCreateService,
} from '../src/tauri/main/services/characters/character-create-service.js';
import { formDataToCreateCharacterDto, payloadToCreateCharacterDto } from '../src/tauri/main/services/characters/character-create-mapper.js';
import { createCharacterFormService } from '../src/tauri/main/services/characters/character-form-service.js';

test('/api/characters/edit-avatar delegates multipart avatar replacement only', async () => {
    const router = createRouteRegistry();
    const calls = [];
    let invalidated = false;
    const context = {
        editCharacterAvatarFromForm: async (formData, url) => {
            calls.push({ formData, url });
        },
        invalidateCharacterCache: () => {
            invalidated = true;
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
    assert.equal(invalidated, true);
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

test('character form edit preserves the embedded lorebook while updating ordinary fields', async () => {
    const calls = [];
    const service = createCharacterFormService({
        safeInvoke: async (command, args) => calls.push({ command, args }),
        resolveCharacterId: async () => 'Alice',
        resolveExistingCharacterId: async () => 'Alice',
        materializeUploadFile: async () => {
            throw new Error('materializeUploadFile should not be called');
        },
    });
    const characterBook = {
        name: 'Embedded Lore',
        entries: [{ keys: ['alpha'], content: 'kept' }],
    };
    const body = new FormData();
    body.set('avatar_url', 'Alice.png');
    body.set('ch_name', 'Alice');
    body.set('world', 'Local Lore');
    body.set('description', 'updated');
    body.set('json_data', JSON.stringify({
        data: {
            character_book: characterBook,
            extensions: { world: 'Local Lore' },
        },
    }));

    await service.editCharacterFromForm(body, new URL('http://localhost/api/characters/edit'));

    assert.equal(calls.length, 1);
    const written = JSON.parse(calls[0].args.dto.card_json);
    assert.deepEqual(written.data.character_book, characterBook);
    assert.equal(written.data.description, 'updated');
    assert.equal(calls[0].args.dto.materialize_primary_lorebook, true);
});

test('character form ignores malformed optional card JSON and saves owned fields', async () => {
    const calls = [];
    const service = createCharacterFormService({
        safeInvoke: async (command, args) => calls.push({ command, args }),
        resolveCharacterId: async () => 'Alice',
        resolveExistingCharacterId: async () => 'Alice',
        materializeUploadFile: async () => {
            throw new Error('materializeUploadFile should not be called');
        },
    });
    const body = new FormData();
    body.set('avatar_url', 'Alice.png');
    body.set('ch_name', 'Alice');
    body.set('description', 'recovered');
    body.set('json_data', '{');
    body.set('extensions', '[');

    await service.editCharacterFromForm(body, new URL('http://localhost/api/characters/edit'));

    assert.equal(calls.length, 1);
    const written = JSON.parse(calls[0].args.dto.card_json);
    assert.equal(written.data.description, 'recovered');
    assert.equal(written.spec, 'chara_card_v2');
});

test('/api/characters/create accepts upstream JSON character payloads', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        createCharacterFromForm: async () => {
            throw new Error('createCharacterFromForm should not be called');
        },
        createCharacterFromPayload: async (payload) => {
            calls.push({ type: 'payload', payload });
            return { character: { avatar: 'Alice.png' }, warnings: [] };
        },
        invalidateCharacterCache: () => calls.push({ type: 'invalidate' }),
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const payload = {
        ch_name: 'Alice',
        description: 'A friendly assistant',
        first_mes: 'Hello',
        world: 'Shared Lore',
        extensions: '{}',
    };

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/create',
        url: new URL('http://localhost/api/characters/create'),
        body: payload,
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(await response.text(), 'Alice.png');
    assert.deepEqual(calls, [
        { type: 'payload', payload },
        { type: 'invalidate' },
    ]);
});

test('/api/characters/create keeps text body and exposes avatar fallback warning header', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        createCharacterFromForm: async (formData, url) => {
            calls.push({ type: 'form', formData, url });
            return {
                character: { avatar: 'Alice.png' },
                warnings: [{
                    code: CHARACTER_CREATE_WARNINGS.AVATAR_IMPORT_FAILED,
                    message: 'Unable to access avatar file path: simulated failure',
                }],
            };
        },
        createCharacterFromPayload: async () => {
            throw new Error('createCharacterFromPayload should not be called');
        },
        invalidateCharacterCache: () => calls.push({ type: 'invalidate' }),
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const body = new FormData();
    body.set('ch_name', 'Alice');
    body.set('avatar', new Blob(['avatar-bytes'], { type: 'image/png' }), 'avatar.png');
    const url = new URL('http://localhost/api/characters/create');

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/create',
        url,
        body,
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(await response.text(), 'Alice.png');
    assert.equal(response.headers.get('x-tauritavern-warning'), CHARACTER_CREATE_WARNINGS.AVATAR_IMPORT_FAILED);
    assert.deepEqual(calls, [
        { type: 'form', formData: body, url },
        { type: 'invalidate' },
    ]);
});

test('/api/characters/duplicate rejects path-like avatar_url before resolving characters', async () => {
    const router = createRouteRegistry();
    const context = {
        resolveExistingCharacterId: async () => {
            throw new Error('resolveExistingCharacterId should not be called');
        },
        safeInvoke: async () => {
            throw new Error('safeInvoke should not be called');
        },
        normalizeCharacter: (character) => character,
        getAllCharacters: async () => [],
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/duplicate',
        url: new URL('http://localhost/api/characters/duplicate'),
        body: { avatar_url: '../Alice.png' },
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.deepEqual(await response.json(), { error: 'invalid avatar_url' });
});


test('/api/characters/delete returns 400 for URL-like avatar_url without backend mutation', async () => {
    const router = createRouteRegistry();
    const context = {
        resolveCharacterId: async () => {
            throw new Error('Bad request: invalid avatar_url');
        },
        safeInvoke: async () => {
            throw new Error('safeInvoke should not be called');
        },
        getAllCharacters: async () => [],
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/delete',
        url: new URL('http://localhost/api/characters/delete'),
        body: { avatar_url: 'Alice.png#hash', name: 'Alice' },
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.deepEqual(await response.json(), { error: 'invalid avatar_url' });
});




test('/api/characters/merge-attributes rejects invalid bulk avatar filenames before backend work', async () => {
    const router = createRouteRegistry();
    const context = {
        safeInvoke: async () => {
            throw new Error('safeInvoke should not be called');
        },
        getAllCharacters: async () => [],
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/merge-attributes',
        url: new URL('http://localhost/api/characters/merge-attributes'),
        body: {
            avatars: ['Alice.png', 'Alice.png?cache=1'],
            data: { data: { description: 'Updated' } },
        },
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.deepEqual(await response.json(), { error: 'invalid avatars[1]' });
});






test('character create mapper only materializes flat world payloads', () => {
    const dto = payloadToCreateCharacterDto({
        ch_name: 'Alice',
        extensions: JSON.stringify({ world: 'extension-book' }),
    });

    assert.equal(dto.primary_lorebook, null);
    assert.equal(dto.extensions.world, 'extension-book');
});




test('character create service uses default avatar when upload materialization fails', async () => {
    const invokes = [];
    const originalWarn = console.warn;
    const warnings = [];
    console.warn = (...args) => warnings.push(args);

    try {
        const service = createCharacterCreateService({
            safeInvoke: async (command, args) => {
                invokes.push({ command, args });
                if (command !== 'create_character') {
                    throw new Error(`unexpected command: ${command}`);
                }
                return { avatar: 'Assistant.png' };
            },
            materializeUploadFile: async (file, options) => {
                assert.ok(file instanceof Blob);
                assert.deepEqual(options, { kind: 'avatar', preferredName: 'assistant.png' });
                return {
                    filePath: '',
                    error: 'simulated temp write failure',
                    isTemporary: false,
                };
            },
        });

        const formData = new FormData();
        formData.set('file_name', 'Assistant');
        formData.set('ch_name', 'Neutral Assistant');
        formData.set('avatar', new Blob(['avatar-bytes'], { type: 'image/png' }), 'assistant.png');

        const outcome = await service.createCharacterFromForm(
            formData,
            new URL('http://localhost/api/characters/create'),
        );

        assert.deepEqual(outcome, {
            character: { avatar: 'Assistant.png' },
            warnings: [{
                code: CHARACTER_CREATE_WARNINGS.AVATAR_IMPORT_FAILED,
                message: 'Unable to access avatar file path: simulated temp write failure',
            }],
        });
        assert.deepEqual(invokes, [
            {
                command: 'create_character',
                args: {
                    dto: {
                        file_name: 'Assistant',
                        json_data: null,
                        primary_lorebook: null,
                        name: 'Neutral Assistant',
                        description: '',
                        personality: '',
                        scenario: '',
                        first_mes: '',
                        mes_example: '',
                        creator: '',
                        creator_notes: '',
                        character_version: '',
                        tags: [],
                        talkativeness: 0.5,
                        fav: false,
                        alternate_greetings: [],
                        system_prompt: '',
                        post_history_instructions: '',
                        extensions: {
                            world: '',
                            depth_prompt: {
                                prompt: '',
                                depth: 4,
                                role: 'system',
                            },
                            talkativeness: 0.5,
                            fav: false,
                        },
                    },
                },
            },
        ]);
        assert.equal(warnings.length, 1);
    } finally {
        console.warn = originalWarn;
    }
});
