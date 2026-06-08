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
        getAllCharacters: async (options) => {
            calls.push({ type: 'refresh', options });
            return [];
        },
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
        { type: 'refresh', options: { shallow: true, forceRefresh: true } },
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
        getAllCharacters: async (options) => {
            calls.push({ type: 'refresh', options });
            return [];
        },
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
        { type: 'refresh', options: { shallow: true, forceRefresh: true } },
    ]);
});

test('/api/characters/create exposes invalid JSON payloads as host bad requests', () => {
    assert.throws(
        () => payloadToCreateCharacterDto({ description: 'missing name' }),
        /Bad request: Character name is required/,
    );

    const message = 'Bad request: Character name is required';
    assert.deepEqual(resolveHostErrorResponse(message), {
        status: 400,
        body: message,
    });
});

test('/api/characters/duplicate delegates to backend file-copy semantics', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        resolveExistingCharacterId: async ({ avatar }) => {
            calls.push({ type: 'resolve', avatar });
            return 'Alice';
        },
        safeInvoke: async (command, args) => {
            calls.push({ type: 'invoke', command, args });
            return { name: 'Alice', avatar: 'Alice_1.png' };
        },
        normalizeCharacter: (character) => {
            calls.push({ type: 'normalize', character });
            return character;
        },
        getAllCharacters: async (options) => {
            calls.push({ type: 'refresh', options });
            return [];
        },
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/duplicate',
        url: new URL('http://localhost/api/characters/duplicate'),
        body: { avatar_url: 'Alice.png' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { path: 'Alice_1.png' });
    assert.deepEqual(calls, [
        { type: 'resolve', avatar: 'Alice.png' },
        {
            type: 'invoke',
            command: 'duplicate_character',
            args: { dto: { name: 'Alice' } },
        },
        { type: 'normalize', character: { name: 'Alice', avatar: 'Alice_1.png' } },
        { type: 'refresh', options: { shallow: true, forceRefresh: true } },
    ]);
});

test('/api/characters/duplicate rejects missing or unknown avatars at the route boundary', async () => {
    const router = createRouteRegistry();
    const context = {
        resolveExistingCharacterId: async () => null,
        safeInvoke: async () => {
            throw new Error('safeInvoke should not be called');
        },
        normalizeCharacter: (character) => character,
        getAllCharacters: async () => [],
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const missingAvatarResponse = await router.handle({
        method: 'POST',
        path: '/api/characters/duplicate',
        url: new URL('http://localhost/api/characters/duplicate'),
        body: {},
    });
    assert.ok(missingAvatarResponse);
    assert.equal(missingAvatarResponse.status, 400);
    assert.deepEqual(await missingAvatarResponse.json(), { error: 'avatar URL not found' });

    const unknownAvatarResponse = await router.handle({
        method: 'POST',
        path: '/api/characters/duplicate',
        url: new URL('http://localhost/api/characters/duplicate'),
        body: { avatar_url: 'Missing.png' },
    });
    assert.ok(unknownAvatarResponse);
    assert.equal(unknownAvatarResponse.status, 404);
    assert.deepEqual(await unknownAvatarResponse.json(), { error: 'Character not found' });
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

test('/api/characters/delete resolves avatar_url as an exact filename before invoking backend delete', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        resolveCharacterId: async ({ avatar, fallbackName }) => {
            calls.push({ type: 'resolve', avatar, fallbackName });
            return 'Alice#1';
        },
        safeInvoke: async (command, args) => {
            calls.push({ type: 'invoke', command, args });
        },
        getAllCharacters: async (options) => {
            calls.push({ type: 'refresh', options });
            return [];
        },
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/delete',
        url: new URL('http://localhost/api/characters/delete'),
        body: { avatar_url: 'Alice#1.png', name: 'Alice', delete_chats: true },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(calls, [
        { type: 'resolve', avatar: 'Alice#1.png', fallbackName: 'Alice' },
        {
            type: 'invoke',
            command: 'delete_character',
            args: { dto: { name: 'Alice#1', delete_chats: true } },
        },
        { type: 'refresh', options: { shallow: true, forceRefresh: true } },
    ]);
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

test('/api/characters/export uses exact avatar filename identities for backend export', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        resolveCharacterId: async ({ avatar, fallbackName }) => {
            calls.push({ type: 'resolve', avatar, fallbackName });
            return 'Alice%2FB';
        },
        safeInvoke: async (command, args) => {
            calls.push({ type: 'invoke', command, args });
            return {
                data: new TextEncoder().encode('{"name":"Alice"}'),
                mime_type: 'application/json',
            };
        },
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/export',
        url: new URL('http://localhost/api/characters/export'),
        body: { avatar_url: 'Alice%2FB.png', name: 'Alice', format: 'json' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(calls, [
        { type: 'resolve', avatar: 'Alice%2FB.png', fallbackName: 'Alice' },
        {
            type: 'invoke',
            command: 'export_character_content',
            args: { dto: { name: 'Alice%2FB', format: 'json' } },
        },
    ]);
    assert.equal(await response.text(), '{"name":"Alice"}');
});

test('/api/characters/merge-attributes supports upstream bulk mode', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        safeInvoke: async (command, args) => {
            calls.push({ type: 'invoke', command, args });
            return { updated: ['Alice.png'], skipped: [], failed: [] };
        },
        getAllCharacters: async (options) => {
            calls.push({ type: 'refresh', options });
            return [];
        },
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const body = {
        avatars: ['Alice.png'],
        data: { data: { extensions: { greeting_tools: '__@@UNSET@@__' } } },
        filter: { path: 'data.extensions.greeting_tools' },
    };
    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/merge-attributes',
        url: new URL('http://localhost/api/characters/merge-attributes'),
        body,
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { updated: ['Alice.png'], skipped: [], failed: [] });
    assert.deepEqual(calls, [
        {
            type: 'invoke',
            command: 'bulk_merge_character_card_data',
            args: {
                dto: {
                    avatars: ['Alice.png'],
                    data: { data: { extensions: { greeting_tools: '__@@UNSET@@__' } } },
                    filter: { path: 'data.extensions.greeting_tools' },
                },
            },
        },
        { type: 'refresh', options: { shallow: true, forceRefresh: true } },
    ]);
});

test('/api/characters/merge-attributes preserves exact avatar filenames in single mode', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        resolveCharacterId: async ({ avatar, fallbackName }) => {
            calls.push({ type: 'resolve', avatar, fallbackName });
            return 'Alice#1';
        },
        safeInvoke: async (command, args) => {
            calls.push({ type: 'invoke', command, args });
        },
        getAllCharacters: async (options) => {
            calls.push({ type: 'refresh', options });
            return [];
        },
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/merge-attributes',
        url: new URL('http://localhost/api/characters/merge-attributes'),
        body: { avatar: 'Alice#1.png', name: 'Alice', data: { description: 'Updated' } },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(calls, [
        { type: 'resolve', avatar: 'Alice#1.png', fallbackName: 'Alice' },
        {
            type: 'invoke',
            command: 'merge_character_card_data',
            args: {
                name: 'Alice#1',
                dto: {
                    update: {
                        avatar: 'Alice#1.png',
                        name: 'Alice',
                        data: { description: 'Updated' },
                    },
                },
            },
        },
        { type: 'refresh', options: { shallow: true, forceRefresh: true } },
    ]);
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

test('/api/characters/merge-attributes rejects path-like single avatar fields', async () => {
    const router = createRouteRegistry();
    const context = {
        safeInvoke: async () => {
            throw new Error('safeInvoke should not be called');
        },
        resolveCharacterId: async () => {
            throw new Error('resolveCharacterId should not be called');
        },
        getAllCharacters: async () => [],
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/merge-attributes',
        url: new URL('http://localhost/api/characters/merge-attributes'),
        body: { avatar: 'characters/Alice.png', data: { description: 'Updated' } },
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.deepEqual(await response.json(), { error: 'invalid avatar' });
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

test('character create mapper normalizes JSON and FormData payloads to one DTO contract', () => {
    const payload = {
        file_name: 'AliceCard',
        ch_name: 'Alice',
        description: 'A friendly assistant',
        first_mes: 'Hello',
        tags: ['friend', 'assistant'],
        talkativeness: '0.75',
        fav: 'true',
        world: 'Shared Lore',
        depth_prompt_prompt: 'Stay concise',
        depth_prompt_depth: '6',
        depth_prompt_role: 'assistant',
        extensions: '{}',
    };

    const formData = new FormData();
    for (const [key, value] of Object.entries(payload)) {
        if (Array.isArray(value)) {
            for (const item of value) {
                formData.append('tags[]', item);
            }
        } else {
            formData.set(key, value);
        }
    }

    assert.deepEqual(formDataToCreateCharacterDto(formData), payloadToCreateCharacterDto(payload));
});

test('character create mapper preserves upstream json_data payloads', () => {
    const jsonData = {
        x_custom_top: { nested: true },
        data: {
            x_custom_data: 123,
            character_book: { name: 'embedded-book', entries: [] },
        },
    };

    const dto = payloadToCreateCharacterDto({
        ch_name: 'Alice',
        description: 'Updated',
        json_data: JSON.stringify(jsonData),
    });

    assert.deepEqual(JSON.parse(dto.json_data), jsonData);
});

test('character create mapper fails fast on invalid file_name', () => {
    assert.throws(
        () => payloadToCreateCharacterDto({ ch_name: 'Alice', file_name: '../Alice' }),
        /Bad request: invalid file_name/,
    );
});

test('character create mapper only materializes flat world payloads', () => {
    const dto = payloadToCreateCharacterDto({
        ch_name: 'Alice',
        extensions: JSON.stringify({ world: 'extension-book' }),
    });

    assert.equal(dto.primary_lorebook, null);
    assert.equal(dto.extensions.world, 'extension-book');
});

test('character create service maps upstream JSON create payload to create_character', async () => {
    const invokes = [];
    const service = createCharacterCreateService({
        safeInvoke: async (command, args) => {
            invokes.push({ command, args });
            return { avatar: 'Alice.png' };
        },
        materializeUploadFile: async () => {
            throw new Error('should not be called');
        },
    });

    const outcome = await service.createCharacterFromPayload({
        file_name: 'AliceCard',
        ch_name: 'Alice',
        description: 'A friendly assistant',
        first_mes: 'Hello',
        tags: ['friend', 'assistant'],
        talkativeness: '0.75',
        fav: 'true',
        world: 'Shared Lore',
        depth_prompt_prompt: 'Stay concise',
        depth_prompt_depth: '6',
        depth_prompt_role: 'assistant',
        extensions: '{}',
    });

    assert.deepEqual(outcome, { character: { avatar: 'Alice.png' }, warnings: [] });
    assert.deepEqual(invokes, [
        {
            command: 'create_character',
            args: {
                dto: {
                    file_name: 'AliceCard',
                    json_data: null,
                    primary_lorebook: 'Shared Lore',
                    name: 'Alice',
                    description: 'A friendly assistant',
                    personality: '',
                    scenario: '',
                    first_mes: 'Hello',
                    mes_example: '',
                    creator: '',
                    creator_notes: '',
                    character_version: '',
                    tags: ['friend', 'assistant'],
                    talkativeness: 0.75,
                    fav: true,
                    alternate_greetings: [],
                    system_prompt: '',
                    post_history_instructions: '',
                    extensions: {
                        world: 'Shared Lore',
                        depth_prompt: {
                            prompt: 'Stay concise',
                            depth: 6,
                            role: 'assistant',
                        },
                        talkativeness: 0.75,
                        fav: true,
                    },
                },
            },
        },
    ]);
});

test('character create service maps multipart creates with avatar to create_character_with_avatar', async () => {
    const invokes = [];
    const cleanups = [];
    const service = createCharacterCreateService({
        safeInvoke: async (command, args) => {
            invokes.push({ command, args });
            return { avatar: 'Assistant.png' };
        },
        materializeUploadFile: async (file, options) => {
            assert.ok(file instanceof Blob);
            assert.deepEqual(options, { preferredName: 'assistant.png' });
            return {
                filePath: '/tmp/assistant.png',
                cleanup: async () => cleanups.push('assistant.png'),
            };
        },
    });

    const formData = new FormData();
    formData.set('file_name', 'Assistant');
    formData.set('ch_name', 'Neutral Assistant');
    formData.set('creator_notes', 'Automatically created character.');
    formData.set('avatar', new Blob(['avatar-bytes'], { type: 'image/png' }), 'assistant.png');

    const outcome = await service.createCharacterFromForm(
        formData,
        new URL('http://localhost/api/characters/create?crop=%7B%22x%22%3A0%2C%22y%22%3A0%2C%22width%22%3A300%2C%22height%22%3A400%2C%22want_resize%22%3Atrue%7D'),
    );

    assert.deepEqual(outcome, { character: { avatar: 'Assistant.png' }, warnings: [] });
    assert.deepEqual(invokes, [
        {
            command: 'create_character_with_avatar',
            args: {
                dto: {
                    character: {
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
                        creator_notes: 'Automatically created character.',
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
                    avatar_path: '/tmp/assistant.png',
                    crop: {
                        x: 0,
                        y: 0,
                        width: 300,
                        height: 400,
                        want_resize: true,
                    },
                },
            },
        },
    ]);
    assert.deepEqual(cleanups, ['assistant.png']);
});

test('character create service propagates Rust avatar fallback warnings', async () => {
    const service = createCharacterCreateService({
        safeInvoke: async (command) => {
            assert.equal(command, 'create_character_with_avatar');
            return {
                character: { avatar: 'Assistant.png' },
                warnings: [{
                    code: CHARACTER_CREATE_WARNINGS.AVATAR_IMPORT_FAILED,
                    message: 'Uploaded avatar could not be processed; default avatar was used.',
                }],
            };
        },
        materializeUploadFile: async () => ({
            filePath: '/tmp/assistant.png',
        }),
    });

    const formData = new FormData();
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
            message: 'Uploaded avatar could not be processed; default avatar was used.',
        }],
    });
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
                assert.deepEqual(options, { preferredName: 'assistant.png' });
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
            assert.equal(command, 'get_character');
            assert.deepEqual(args, { name: 'Missing' });
            throw new Error('Not found: Character not found: Missing');
        },
    });

    assert.equal(await service.resolveExistingCharacterId({ avatar: 'Missing.png' }), null);
    assert.equal(await service.resolveCharacterId({ avatar: 'Missing.png' }), 'Missing');
    assert.deepEqual(invokes, [
        { command: 'get_character', args: { name: 'Missing' } },
    ]);
});

test('character service strict resolver verifies exact avatar ids without list refreshes', async () => {
    const invokes = [];
    const service = createCharacterService({
        safeInvoke: async (command, args) => {
            invokes.push({ command, args });
            assert.equal(command, 'get_character');
            if (args.name === 'Alice') {
                return { name: 'Alice', avatar: 'Alice.png', data: { extensions: {} } };
            }
            throw new Error(`Not found: Character not found: ${args.name}`);
        },
    });

    assert.equal(await service.resolveExistingCharacterId({ avatar: 'Alice.png' }), 'Alice');
    assert.equal(await service.resolveExistingCharacterId({ avatar: 'Missing.png' }), null);
    assert.deepEqual(invokes, [
        { command: 'get_character', args: { name: 'Alice' } },
        { command: 'get_character', args: { name: 'Missing' } },
    ]);
});
