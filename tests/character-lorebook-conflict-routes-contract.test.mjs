import assert from 'node:assert/strict';
import test from 'node:test';

import { textResponse, jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerCharacterRoutes } from '../src/tauri/main/routes/character-routes.js';

test('/api/characters/lorebook-conflict resolves avatar identity before checking', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        resolveCharacterId: async ({ avatar, fallbackName }) => {
            calls.push({ type: 'resolve', avatar, fallbackName });
            return 'Alice';
        },
        safeInvoke: async (command, args) => {
            calls.push({ type: 'invoke', command, args });
            return {
                conflict: true,
                world: 'Alice Lore',
                embedded_name: 'Embedded Lore',
                current_available: true,
                conflict_token: 'token-1',
            };
        },
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/lorebook-conflict',
        url: new URL('http://localhost/api/characters/lorebook-conflict'),
        body: { avatar_url: 'Alice.png', name: 'Ignored Fallback' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
        conflict: true,
        world: 'Alice Lore',
        embedded_name: 'Embedded Lore',
        current_available: true,
        conflict_token: 'token-1',
    });
    assert.deepEqual(calls, [
        { type: 'resolve', avatar: 'Alice.png', fallbackName: 'Ignored Fallback' },
        {
            type: 'invoke',
            command: 'check_character_lorebook_conflict',
            args: { dto: { name: 'Alice' } },
        },
    ]);
});

test('/api/characters/resolve-lorebook-conflict maps copy resolution and refreshes character cache', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        resolveCharacterId: async ({ avatar, fallbackName }) => {
            calls.push({ type: 'resolve', avatar, fallbackName });
            return 'Alice';
        },
        safeInvoke: async (command, args) => {
            calls.push({ type: 'invoke', command, args });
            return {
                world: 'Alice Lore',
                affected_world: 'Alice Lore (2)',
                world_written: true,
            };
        },
        getAllCharacters: async (options) => {
            calls.push({ type: 'refresh', options });
            return [];
        },
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/resolve-lorebook-conflict',
        url: new URL('http://localhost/api/characters/resolve-lorebook-conflict'),
        body: {
            avatar_url: 'Alice.png',
            name: 'Ignored Fallback',
            resolution: 'copy',
            conflict_token: 'token-1',
        },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
        world: 'Alice Lore',
        affected_world: 'Alice Lore (2)',
        world_written: true,
    });
    assert.deepEqual(calls, [
        { type: 'resolve', avatar: 'Alice.png', fallbackName: 'Ignored Fallback' },
        {
            type: 'invoke',
            command: 'resolve_character_lorebook_conflict',
            args: {
                dto: {
                    name: 'Alice',
                    resolution: 'copy',
                    conflict_token: 'token-1',
                },
            },
        },
        { type: 'refresh', options: { shallow: true, forceRefresh: true } },
    ]);
});

test('/api/characters/resolve-lorebook-conflict rejects invalid resolutions before backend work', async () => {
    const router = createRouteRegistry();
    const context = {
        resolveCharacterId: async () => {
            throw new Error('resolveCharacterId should not be called');
        },
        safeInvoke: async () => {
            throw new Error('safeInvoke should not be called');
        },
        getAllCharacters: async () => {
            throw new Error('getAllCharacters should not be called');
        },
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/resolve-lorebook-conflict',
        url: new URL('http://localhost/api/characters/resolve-lorebook-conflict'),
        body: { avatar_url: 'Alice.png', resolution: 'latest' },
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.deepEqual(await response.json(), { error: 'Invalid lorebook conflict resolution' });
});

test('/api/characters/resolve-lorebook-conflict requires the checked conflict token', async () => {
    const router = createRouteRegistry();
    const context = {
        resolveCharacterId: async () => {
            throw new Error('resolveCharacterId should not be called');
        },
        safeInvoke: async () => {
            throw new Error('safeInvoke should not be called');
        },
    };
    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/resolve-lorebook-conflict',
        url: new URL('http://localhost/api/characters/resolve-lorebook-conflict'),
        body: { avatar_url: 'Alice.png', resolution: 'copy' },
    });

    assert.equal(response.status, 400);
    assert.deepEqual(await response.json(), { error: 'Missing lorebook conflict token' });
});

test('/api/characters/resolve-lorebook-conflict keeps legacy current resolution compatible', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        resolveCharacterId: async () => 'Alice',
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { world: 'Alice Lore', affected_world: null, world_written: false };
        },
        getAllCharacters: async () => [],
    };
    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/resolve-lorebook-conflict',
        url: new URL('http://localhost/api/characters/resolve-lorebook-conflict'),
        body: { avatar_url: 'Alice.png', resolution: 'current' },
    });

    assert.equal(response.status, 200);
    assert.deepEqual(calls[0].args.dto, {
        name: 'Alice',
        resolution: 'current',
        conflict_token: null,
    });
});
