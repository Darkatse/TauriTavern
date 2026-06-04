import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { readFile } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(relativePath) {
    const modulePath = path.join(REPO_ROOT, relativePath);
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

test('preset rename retargets Agent Profile preset refs through Host API', async () => {
    const calls = [];
    globalThis.window = {
        __TAURITAVERN__: {
            api: {
                agent: {
                    profiles: {
                        async retargetPresetRefs(request) {
                            calls.push(request);
                            return { updated: 1, profileIds: ['writer'] };
                        },
                    },
                },
            },
        },
    };

    const { retargetAgentProfilesAfterPresetRename } = await importFresh(
        'src/scripts/tauritavern/agent/profile-preset-sync.js',
    );

    const result = await retargetAgentProfilesAfterPresetRename({
        apiId: 'openai',
        oldName: 'Old Preset',
        newName: 'New Preset',
    });

    assert.deepEqual(calls, [{
        from: { apiId: 'openai', name: 'Old Preset' },
        to: { apiId: 'openai', name: 'New Preset' },
    }]);
    assert.deepEqual(result, { updated: 1, profileIds: ['writer'] });
});

test('preset rename profile retarget is a no-op before the Host API is installed', async () => {
    globalThis.window = {};

    const { retargetAgentProfilesAfterPresetRename } = await importFresh(
        'src/scripts/tauritavern/agent/profile-preset-sync.js',
    );

    assert.equal(await retargetAgentProfilesAfterPresetRename({
        apiId: 'openai',
        oldName: 'Old Preset',
        newName: 'New Preset',
    }), null);
});

test('preset rename retargets profiles before skills and old preset deletion', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/preset-manager.js'), 'utf8');
    const saveIndex = source.indexOf('await this.savePreset(newName);');
    const profileIndex = source.indexOf('await retargetAgentProfilesAfterPresetRename');
    const skillIndex = source.indexOf('await retargetPresetSkillsAfterRename');
    const deleteIndex = source.indexOf('await this.deletePreset(oldName);');

    assert.ok(saveIndex >= 0, 'renamePreset must save the new preset first');
    assert.ok(profileIndex > saveIndex, 'Agent Profile refs must be retargeted after new preset save');
    assert.ok(skillIndex > profileIndex, 'Preset Skills must be retargeted after Agent Profile refs');
    assert.ok(deleteIndex > skillIndex, 'old preset deletion must happen after dependent refs retarget');
});
