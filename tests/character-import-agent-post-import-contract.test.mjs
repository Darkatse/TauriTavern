import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse, textResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerCharacterRoutes } from '../src/tauri/main/routes/character-routes.js';

test('/api/characters/import returns canonical character payload and Agent post-import hints', async () => {
    const router = createRouteRegistry();
    const imported = { name: 'Alice', avatar: 'Alice.png' };
    const normalized = {
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
    const calls = [];
    const context = {
        materializeUploadFile: async (file, options) => {
            calls.push({
                type: 'materialize',
                fileName: file.name,
                options,
            });
            return {
                filePath: '/tmp/Alice.png',
                cleanup: async () => calls.push({ type: 'cleanup' }),
            };
        },
        safeInvoke: async (command, args) => {
            calls.push({ type: 'invoke', command, args });
            return imported;
        },
        normalizeCharacter: (character) => {
            calls.push({ type: 'normalize', character });
            return normalized;
        },
        invalidateCharacterCache: () => calls.push({ type: 'invalidate' }),
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

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
        file_name: 'Alice',
        replaced: false,
        character: normalized,
        post_import: {
            has_agent_profiles: true,
            has_agent_skills: true,
        },
    });
    assert.deepEqual(calls, [
        {
            type: 'materialize',
            fileName: 'Alice.png',
            options: {
                kind: 'character-import',
                preferredName: 'Alice.png',
                preferredExtension: 'png',
            },
        },
        {
            type: 'invoke',
            command: 'import_character',
            args: {
                dto: {
                    file_path: '/tmp/Alice.png',
                    preserve_file_name: null,
                },
            },
        },
        { type: 'cleanup' },
        { type: 'normalize', character: imported },
        { type: 'invalidate' },
    ]);
});

for (const [preservedName, stem] of [
    ['Alice', 'Alice'],
    ['Alice.png', 'Alice'],
    ['OZ前端.测试', 'OZ前端.测试'],
    ['OZ前端.测试.png', 'OZ前端.测试'],
    ['Alice#1%2F', 'Alice#1%2F'],
    ['Alice.png.png', 'Alice.png'],
    ['Alice.PNG', 'Alice.PNG'],
    ['Alice.png ', 'Alice.png '],
]) {
    for (const existing of [true, false]) {
        test(`/api/characters/import ${existing ? 'replaces' : 'imports'} preserved name ${JSON.stringify(preservedName)}`, async () => {
            const router = createRouteRegistry();
            const calls = [];
            const imported = { name: 'Card display name', avatar: `${stem}.png` };
            const context = {
                materializeUploadFile: async () => ({
                    filePath: '/tmp/update.png',
                    cleanup: async () => calls.push({ type: 'cleanup' }),
                }),
                safeInvoke: async (command, args) => {
                    calls.push({ type: 'invoke', command, args });
                    if (command === 'replace_character' && !existing) {
                        throw new Error(`Not found: Character not found: ${stem}`);
                    }
                    return imported;
                },
                normalizeCharacter: character => character,
                invalidateCharacterCache: () => calls.push({ type: 'invalidate' }),
            };
            registerCharacterRoutes(router, context, { textResponse, jsonResponse });

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

            assert.equal(response.status, 200);
            const payload = await response.json();
            assert.equal(payload.replaced, existing);
            assert.equal(payload.file_name, stem);
            assert.deepEqual(payload.character, imported);
            assert.deepEqual(calls, [
                {
                    type: 'invoke',
                    command: 'replace_character',
                    args: { dto: { file_path: '/tmp/update.png', name: stem } },
                },
                ...(!existing ? [{
                    type: 'invoke',
                    command: 'import_character',
                    args: { dto: { file_path: '/tmp/update.png', preserve_file_name: `${stem}.png` } },
                }] : []),
                { type: 'cleanup' },
                { type: 'invalidate' },
            ]);
        });
    }
}

test('/api/characters/import rejects unsafe preserved names before staging', async () => {
    const router = createRouteRegistry();
    const context = {
        materializeUploadFile: async () => {
            throw new Error('invalid identity must be rejected before staging');
        },
    };
    registerCharacterRoutes(router, context, { textResponse, jsonResponse });

    for (const preservedName of ['folder/Alice', 'folder/Alice.png', '../Alice', 'folder\\Alice', 'Alice.png?cache=1', 'Alice\0', 'Alice\n', 'Alice:*', '.png', '.', '..', '..png', '...png']) {
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
        assert.deepEqual(await response.json(), { error: 'invalid preserved_name' });
    }
});

for (const failureCommand of ['replace_character', 'import_character']) {
    test(`/api/characters/import cleans staged files after ${failureCommand} fails`, async () => {
        const router = createRouteRegistry();
        const calls = [];
        const failure = new Error('Invalid data: corrupt character card');
        registerCharacterRoutes(router, {
            materializeUploadFile: async () => ({
                filePath: '/tmp/update.png',
                cleanup: async () => calls.push('cleanup'),
            }),
            safeInvoke: async command => {
                calls.push(command);
                if (command === failureCommand) throw failure;
                throw new Error('Not found: Character not found: Alice');
            },
            invalidateCharacterCache: () => calls.push('invalidate'),
        }, { textResponse, jsonResponse });
        const body = new FormData();
        body.set('avatar', new Blob(['bad-card']), 'update.png');
        body.set('preserved_name', 'Alice');

        await assert.rejects(router.handle({
            method: 'POST',
            path: '/api/characters/import',
            url: new URL('http://localhost/api/characters/import'),
            body,
        }), error => error === failure);
        assert.deepEqual(calls, [
            'replace_character',
            ...(failureCommand === 'import_character' ? ['import_character'] : []),
            'cleanup',
        ]);
    });
}
