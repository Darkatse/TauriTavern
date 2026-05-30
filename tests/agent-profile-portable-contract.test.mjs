import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importPortableProfile() {
    return import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/scripts/tauritavern/agent/agent-profile-portable.js',
    )));
}

test('portable Agent profile export strips local model connection bindings', async () => {
    const { sanitizePortableAgentProfile } = await importPortableProfile();
    const profile = {
        schemaVersion: 2,
        kind: 'tauritavern.agentProfile',
        id: 'writer',
        model: {
            mode: 'connectionRef',
            connectionRef: 'model-target-private-main',
            modelId: 'provider/private-model',
        },
    };

    const portable = sanitizePortableAgentProfile(profile);

    assert.deepEqual(portable.model, {
        mode: 'requiresConfiguration',
    });
    assert.equal(profile.model.mode, 'connectionRef');
});

test('portable Agent profile export preserves non-local model modes', async () => {
    const { sanitizePortableAgentProfile } = await importPortableProfile();

    assert.deepEqual(sanitizePortableAgentProfile({
        id: 'ambient',
        model: { mode: 'currentPromptSnapshot' },
    }).model, { mode: 'currentPromptSnapshot' });
    assert.deepEqual(sanitizePortableAgentProfile({
        id: 'already-portable',
        model: { mode: 'requiresConfiguration' },
    }).model, { mode: 'requiresConfiguration' });
});

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

test('Preset export sanitizes embedded Agent profiles before serialization', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/preset-manager.js'), 'utf8');

    assert.match(source, /sanitizePortableAgentProfilePackage/);
    assert.match(source, /const exportPreset = structuredClone\(preset\);/);
    assert.match(source, /const exportPreset = buildPortablePresetForExport\(preset\);\s*const data = JSON\.stringify\(exportPreset, null, 4\);/);
});
