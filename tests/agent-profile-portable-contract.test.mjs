import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importPortableProfile() {
    return import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/scripts/tauritavern/agent/agent-profile-portable.js',
    )));
}

test('portable embedded Agent profile package strips local model connection bindings', async () => {
    const { sanitizePortableAgentProfilePackage } = await importPortableProfile();
    const packageValue = {
        version: 1,
        items: [
            {
                source: 'preset',
                profile: {
                    id: 'editor',
                    model: {
                        mode: 'connectionRef',
                        connectionRef: 'private-target',
                        modelId: 'private-model',
                    },
                },
            },
            { profile: { id: 'ambient', model: { mode: 'currentPromptSnapshot' } } },
            { profile: { id: 'portable', model: { mode: 'requiresConfiguration' } } },
        ],
    };

    const portable = sanitizePortableAgentProfilePackage(packageValue);

    assert.deepEqual(portable.items[0], {
        source: 'preset',
        profile: {
            id: 'editor',
            model: { mode: 'requiresConfiguration' },
        },
    });
    assert.equal(packageValue.items[0].profile.model.mode, 'connectionRef');
    assert.deepEqual(portable.items.slice(1), packageValue.items.slice(1));
});

test('portable embedded Agent profile package fails fast on malformed items', async () => {
    const { sanitizePortableAgentProfilePackage } = await importPortableProfile();

    assert.throws(
        () => sanitizePortableAgentProfilePackage({ version: 2, items: [] }),
        /Unsupported embedded Agent Profile schema version: 2/,
    );
    assert.throws(
        () => sanitizePortableAgentProfilePackage({ version: 1, items: [{}] }),
        /item\.profile must be an object/,
    );
});
