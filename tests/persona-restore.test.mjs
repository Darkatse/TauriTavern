import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

const DEFAULTS = {
    defaultPosition: 0,
    defaultDepth: 2,
    defaultRole: 0,
};

function createDefaultDescriptor() {
    return {
        description: '',
        position: DEFAULTS.defaultPosition,
        depth: DEFAULTS.defaultDepth,
        role: DEFAULTS.defaultRole,
        lorebook: '',
        title: '',
    };
}




test('persona restore: overwrites placeholder persona name + descriptor (default only)', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/persona-restore.js'));
    const { restorePersonasFromBackup, UNNAMED_PERSONA } = mod;

    const target = {
        personas: {
            'a.png': UNNAMED_PERSONA,
        },
        persona_descriptions: {
            'a.png': createDefaultDescriptor(),
        },
        default_persona: null,
    };

    const backup = {
        personas: {
            'a.png': 'Alice',
        },
        persona_descriptions: {
            'a.png': { ...createDefaultDescriptor(), description: 'restored' },
        },
        default_persona: 'a.png',
    };

    const result = restorePersonasFromBackup(target, backup, DEFAULTS);

    assert.deepEqual(result.warnings, []);
    assert.deepEqual(Array.from(result.restoredPersonas), ['a.png']);
    assert.equal(target.personas['a.png'], 'Alice');
    assert.equal(target.persona_descriptions['a.png'].description, 'restored');
    assert.equal(target.default_persona, 'a.png');
});

test('persona restore: skips non-placeholder conflicts and reports warnings', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/persona-restore.js'));
    const { restorePersonasFromBackup } = mod;

    const target = {
        personas: {
            'a.png': 'Existing',
        },
        persona_descriptions: {
            'a.png': { ...createDefaultDescriptor(), description: 'custom' },
        },
        default_persona: null,
    };

    const backup = {
        personas: {
            'a.png': 'Imported',
        },
        persona_descriptions: {
            'a.png': { ...createDefaultDescriptor(), description: 'imported desc' },
        },
        default_persona: 'a.png',
    };

    const result = restorePersonasFromBackup(target, backup, DEFAULTS);

    assert.deepEqual(Array.from(result.restoredPersonas), []);
    assert.equal(target.personas['a.png'], 'Existing');
    assert.equal(target.persona_descriptions['a.png'].description, 'custom');
    assert.equal(target.default_persona, 'a.png');
    assert.ok(result.warnings.some((warning) => warning.includes('already exists, skipping')));
    assert.ok(result.warnings.some((warning) => warning.includes('Persona description for')));
});


test('persona restore: warns when default persona does not exist after restore', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/persona-restore.js'));
    const { restorePersonasFromBackup } = mod;

    const target = {
        personas: {},
        persona_descriptions: {},
        default_persona: null,
    };

    const backup = {
        personas: {},
        persona_descriptions: {},
        default_persona: 'missing.png',
    };

    const result = restorePersonasFromBackup(target, backup, DEFAULTS);
    assert.equal(result.warnings[0], 'Default persona "missing.png" does not exist, skipping');
});

test('persona restore: normalizes legacy string descriptors', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/persona-restore.js'));
    const { restorePersonasFromBackup } = mod;

    const target = {
        personas: {},
        persona_descriptions: {},
        default_persona: null,
    };

    const backup = {
        personas: {
            'a.png': 'Alice',
        },
        persona_descriptions: {
            'a.png': 'hello',
        },
    };

    const result = restorePersonasFromBackup(target, backup, DEFAULTS);

    assert.deepEqual(result.warnings, []);
    assert.equal(target.persona_descriptions['a.png'].description, 'hello');
    assert.equal(target.persona_descriptions['a.png'].position, DEFAULTS.defaultPosition);
    assert.equal(target.persona_descriptions['a.png'].depth, DEFAULTS.defaultDepth);
    assert.equal(target.persona_descriptions['a.png'].role, DEFAULTS.defaultRole);
});
