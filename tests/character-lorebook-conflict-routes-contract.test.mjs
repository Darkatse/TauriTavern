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

test('/api/characters/resolve-lorebook-conflict maps resolution and refreshes character cache', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        resolveCharacterId: async ({ avatar, fallbackName }) => {
            calls.push({ type: 'resolve', avatar, fallbackName });
            return 'Alice';
        },
        safeInvoke: async (command, args) => {
            calls.push({ type: 'invoke', command, args });
            return { world: 'Alice Lore' };
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
        body: { avatar_url: 'Alice.png', name: 'Ignored Fallback', resolution: 'embedded' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { world: 'Alice Lore' });
    assert.deepEqual(calls, [
        { type: 'resolve', avatar: 'Alice.png', fallbackName: 'Ignored Fallback' },
        {
            type: 'invoke',
            command: 'resolve_character_lorebook_conflict',
            args: { dto: { name: 'Alice', resolution: 'embedded' } },
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
