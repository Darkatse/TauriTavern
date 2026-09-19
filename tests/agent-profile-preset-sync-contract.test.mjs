import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
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
