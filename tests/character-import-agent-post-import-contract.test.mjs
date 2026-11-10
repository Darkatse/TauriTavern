import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse, textResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerCharacterRoutes } from '../src/tauri/main/routes/character-routes.js';

test('/api/characters/import returns character data, lorebook adoption and Agent post-import hints', async () => {
    const router = createRouteRegistry();
    const imported = {
        name: 'Alice',
        avatar: 'Alice.png',
        data: {
            extensions: {
                tauritavern: {
                    agentProfiles: 0,
                },
            },
        },
        extensions: {
            tauritavern: {
                skills: {
                    version: 1,
                    items: [],
                },
            },
        },
    };
    const context = {
        materializeUploadFile: async () => ({ filePath: '/tmp/Alice.png', cleanup: async () => {} }),
        safeInvoke: async () => ({ character: imported, lorebook: { status: 'none' } }),
        normalizeCharacter: character => character,
        invalidateCharacterCache: () => {},
    };

    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const body = new FormData();
    body.set('avatar', new Blob(['png-bytes'], { type: 'image/png' }), 'Alice.png');
    body.set('file_type', 'png');

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/import',
        url: new URL('http://localhost/api/characters/import'),
        body,
    });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
        file_name: 'Alice',
        replaced: false,
        character: imported,
        lorebook: { status: 'none' },
        post_import: {
            has_agent_profiles: true,
            has_agent_skills: true,
        },
    });
});

test('/api/characters/import uses the explicit replacement command for an existing exact avatar', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const imported = { name: 'Updated Alice', avatar: 'Alice.png' };
    const context = {
        materializeUploadFile: async () => ({ filePath: '/tmp/update.png', cleanup: async () => {} }),
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { character: imported, lorebook: { status: 'none' } };
        },
        normalizeCharacter: character => character,
        invalidateCharacterCache: () => {},
    };
    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const body = new FormData();
    body.set('avatar', new Blob(['png-bytes'], { type: 'image/png' }), 'update.png');
    body.set('file_type', 'png');
    body.set('preserved_name', 'Alice.png');
    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/import',
        url: new URL('http://localhost/api/characters/import'),
        body,
    });

    assert.equal(response.status, 200);
    assert.equal((await response.json()).replaced, true);
    assert.deepEqual(calls, [
        {
            command: 'replace_character',
            args: { dto: { file_path: '/tmp/update.png', name: 'Alice' } },
        },
    ]);
});

test('/api/characters/import replaces from a host-staged file by path and releases it on failure (#428)', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const discarded = [];
    const context = {
        materializeUploadFile: async () => {
            throw new Error('a staged file must not be uploaded again');
        },
        adoptStagedFile: filePath => ({ filePath, cleanup: async () => { discarded.push(filePath); } }),
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            throw new Error('Bad request: corrupt card');
        },
    };
    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const body = new FormData();
    body.set('staged_path', '/cache/character-card/0f3a.png');
    body.set('file_type', 'png');
    body.set('preserved_name', 'Alice.png');
    await assert.rejects(router.handle({
        method: 'POST',
        path: '/api/characters/import',
        url: new URL('http://localhost/api/characters/import'),
        body,
    }), /corrupt card/);

    assert.deepEqual(calls, [{
        command: 'replace_character',
        args: { dto: { file_path: '/cache/character-card/0f3a.png', name: 'Alice' } },
    }]);
    assert.deepEqual(discarded, ['/cache/character-card/0f3a.png']);
});

test('/api/characters/import preserves an exact name when there is no character to replace', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const imported = { name: 'Alice', avatar: 'Alice.png' };
    const context = {
        materializeUploadFile: async () => ({ filePath: '/tmp/Alice.png', cleanup: async () => {} }),
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'replace_character') {
                throw new Error('Not found: Character not found: Alice');
            }
            return { character: imported, lorebook: { status: 'none' } };
        },
        normalizeCharacter: character => character,
        invalidateCharacterCache: () => {},
    };
    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    const body = new FormData();
    body.set('avatar', new Blob(['png-bytes'], { type: 'image/png' }), 'Alice.png');
    body.set('file_type', 'png');
    body.set('preserved_name', 'Alice.png');
    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/import',
        url: new URL('http://localhost/api/characters/import'),
        body,
    });

    assert.equal(response.status, 200);
    assert.equal((await response.json()).replaced, false);
    assert.deepEqual(calls, [
        {
            command: 'replace_character',
            args: { dto: { file_path: '/tmp/Alice.png', name: 'Alice' } },
        },
        {
            command: 'import_character',
            args: {
                dto: {
                    file_path: '/tmp/Alice.png',
                    preserve_file_name: 'Alice.png',
                },
            },
        },
    ]);
});

test('/api/characters/import rejects non-exact preserved avatar identities before staging', async () => {
    const router = createRouteRegistry();
    const context = {
        materializeUploadFile: async () => {
            throw new Error('invalid identity must be rejected before staging');
        },
    };
    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    for (const preservedName of ['Alice.PNG', 'Alice.png ', 'folder/Alice.png', 'Alice.png?cache=1']) {
        const body = new FormData();
        body.set('avatar', new Blob(['png-bytes'], { type: 'image/png' }), 'update.png');
        body.set('file_type', 'png');
        body.set('preserved_name', preservedName);
        const response = await router.handle({
            method: 'POST',
            path: '/api/characters/import',
            url: new URL('http://localhost/api/characters/import'),
            body,
        });

        assert.equal(response.status, 400, preservedName);
    }
});
