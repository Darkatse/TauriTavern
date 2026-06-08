import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { jsonResponse, textResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerCharacterRoutes } from '../src/tauri/main/routes/character-routes.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function sliceSource(source, start, end) {
    const startIndex = source.indexOf(start);
    assert.notEqual(startIndex, -1, `Missing source marker: ${start}`);
    const endIndex = source.indexOf(end, startIndex);
    assert.notEqual(endIndex, -1, `Missing source marker: ${end}`);
    return source.slice(startIndex, endIndex);
}

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
        getAllCharacters: async (options) => {
            calls.push({ type: 'refresh', options });
            return [];
        },
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
        { type: 'refresh', options: { shallow: true, forceRefresh: true } },
    ]);
});

test('character import Agent scan uses import payload instead of immediate character reload', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const postprocessSource = await readFile(
        path.join(REPO_ROOT, 'src/scripts/tauri/agent-import-postprocess.js'),
        'utf8',
    );
    const importCharacterSource = sliceSource(
        source,
        'async function importCharacter(file',
        'async function importFromURL',
    );

    assert.match(source, /from '\.\/scripts\/tauri\/agent-import-postprocess\.js';/);
    assert.doesNotMatch(source, /async function maybePromptForImportedCharacterAgentAssets/);
    assert.doesNotMatch(postprocessSource, /\/api\/characters\/get/);
    assert.match(postprocessSource, /if \(!hasProfiles && !hasSkills\) \{\s*return;\s*\}/);
    assert.match(postprocessSource, /const loadCharacter = async \(\) => importedCharacter;/);
    assert.doesNotMatch(importCharacterSource, /await maybePromptForImportedCharacterAgentAssets/);
    assert.match(importCharacterSource, /enqueueImportedCharacterAgentAssetScan\(\{[\s\S]*character:\s*data\.character,[\s\S]*postImport:\s*data\.post_import,/);
});
