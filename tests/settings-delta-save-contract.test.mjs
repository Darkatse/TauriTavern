import test from 'node:test';
import assert from 'node:assert/strict';

import {
    applyPersonaSnapshot,
    buildSettingsPatchSaveRequest,
    captureSettingsSaveBaseline,
    clearSettingsSaveBaseline,
    isSettingsPatchConflictError,
    prepareSettingsSavePayload,
    requireSettingsRevision,
    SETTINGS_HASH_ALGORITHM,
    trySaveSettingsDelta,
} from '../src/scripts/tauri/setting/settings-delta-save.js';

const originalFetch = globalThis.fetch;
const originalWindow = globalThis.window;

const revision = {
    hash_algorithm: SETTINGS_HASH_ALGORITHM,
    settings_hash: 'a'.repeat(64),
};

function restoreGlobals() {
    globalThis.fetch = originalFetch;
    if (originalWindow === undefined) {
        delete globalThis.window;
    } else {
        globalThis.window = originalWindow;
    }
}

test.afterEach(() => {
    clearSettingsSaveBaseline();
    restoreGlobals();
});

test('requireSettingsRevision validates full-save revision responses', () => {
    assert.deepEqual(requireSettingsRevision({ result: 'ok', mode: 'full', ...revision }), revision);
    assert.throws(() => requireSettingsRevision({ result: 'ok' }), /missing revision/);
});


test('buildSettingsPatchSaveRequest emits backend-revision CAS object diffs and replaces arrays whole', () => {
    captureSettingsSaveBaseline({
        filler: 'x'.repeat(1000),
        profile: { age: 1, name: 'old' },
        list: [1, 2],
        obsolete: true,
    }, revision);

    const request = buildSettingsPatchSaveRequest(prepareSettingsSavePayload({
        filler: 'x'.repeat(1000),
        profile: { age: 1, name: 'new' },
        list: [1, 3],
        added: { enabled: true },
    }));

    assert.ok(request);
    assert.equal(request.patch.hash_algorithm, SETTINGS_HASH_ALGORITHM);
    assert.equal(request.patch.base_hash, revision.settings_hash);
    assert.equal(Object.prototype.hasOwnProperty.call(request.patch, 'next_hash'), false);
    assert.deepEqual(request.patch.ops, [
        { op: 'set', path: ['added'], value: { enabled: true } },
        { op: 'set', path: ['list'], value: [1, 3] },
        { op: 'delete', path: ['obsolete'] },
        { op: 'set', path: ['profile', 'name'], value: 'new' },
    ]);
});

test('large settings patches replace the root without rewriting untouched Personas', () => {
    const power_user = {
        personas: { 'keep.png': 'Keep' },
        persona_descriptions: { 'keep.png': { description: 'Keep this text' } },
    };
    const core = { power_user: {} };
    for (let index = 0; index < 300; index++) {
        core[`setting_${index}`] = index;
    }

    captureSettingsSaveBaseline({ power_user }, revision);

    const { patch } = buildSettingsPatchSaveRequest(prepareSettingsSavePayload({ ...core, power_user }));

    assert.deepEqual(patch.ops, [{ op: 'set', path: [], value: core }]);
    assert.deepEqual(patch.persona_updates, {});
});

test('trySaveSettingsDelta sends an empty CAS patch for unchanged settings', async () => {
    globalThis.window = { __TAURI_RUNNING__: true };

    let capturedUrl = '';
    let capturedBody = null;
    const nextRevision = {
        hash_algorithm: SETTINGS_HASH_ALGORITHM,
        settings_hash: 'b'.repeat(64),
    };
    globalThis.fetch = async (url, init) => {
        capturedUrl = url;
        capturedBody = JSON.parse(init.body);
        return new Response(JSON.stringify({ result: 'ok', mode: 'patch-noop', ...nextRevision }), {
            status: 200,
            headers: { 'content-type': 'application/json' },
        });
    };

    captureSettingsSaveBaseline({ a: 1 }, revision);
    const result = await trySaveSettingsDelta(prepareSettingsSavePayload({ a: 1 }), { 'Content-Type': 'application/json' });

    assert.deepEqual(result, { saved: true, mode: 'patch-noop', revision: nextRevision });
    assert.equal(capturedUrl, '/api/settings/patch');
    assert.deepEqual(capturedBody, {
        hash_algorithm: SETTINGS_HASH_ALGORITHM,
        base_hash: revision.settings_hash,
        ops: [],
        persona_updates: {},
    });
});

test('trySaveSettingsDelta surfaces CAS conflicts without full-save fallback', async () => {
    globalThis.window = { __TAURI_RUNNING__: true };
    globalThis.fetch = async () => new Response('Conflict: stale settings revision', { status: 409 });

    captureSettingsSaveBaseline({ a: 1, filler: 'x'.repeat(1000) }, revision);

    await assert.rejects(
        () => trySaveSettingsDelta(prepareSettingsSavePayload({ a: 2, filler: 'x'.repeat(1000) }), {}),
        error => {
            assert.equal(isSettingsPatchConflictError(error), true);
            assert.match(error.message, /stale settings revision/);
            return true;
        },
    );
});


test('Persona refresh preserves pending edits and snapshots received during save', async () => {
    const settings = { power_user: {
        personas: { 'local.png': 'Before', 'remote.png': 'Remote' },
        persona_descriptions: { 'local.png': { description: 'Local description' } },
    } };
    captureSettingsSaveBaseline(settings, revision);
    settings.power_user.personas['local.png'] = 'Edited';
    applyPersonaSnapshot(settings.power_user, {
        'local.png': { name: 'Before', description: settings.power_user.persona_descriptions['local.png'] },
        'remote.png': { name: 'Synced' },
        'new.png': { name: 'New', description: { description: 'From another device' } },
    });
    assert.equal(settings.power_user.personas['local.png'], 'Edited');
    assert.equal(settings.power_user.personas['remote.png'], 'Synced');
    assert.equal(settings.power_user.personas['new.png'], 'New');
    const { patch } = buildSettingsPatchSaveRequest(prepareSettingsSavePayload(settings));
    assert.deepEqual(patch.persona_updates, {
        'local.png': { name: 'Edited', description: settings.power_user.persona_descriptions['local.png'] },
    });
    assert.deepEqual(patch.ops, []);

    globalThis.window = { __TAURI_RUNNING__: true };
    globalThis.fetch = async () => {
        applyPersonaSnapshot(settings.power_user, {
            'local.png': patch.persona_updates['local.png'],
            'arrived.png': { name: 'Arrived during save' },
        });
        return Response.json({ ...revision, mode: 'patch' });
    };
    await trySaveSettingsDelta(prepareSettingsSavePayload(settings), {});
    assert.deepEqual(buildSettingsPatchSaveRequest(prepareSettingsSavePayload(settings)).patch.persona_updates, {});

    // Entries removed from disk disappear without generating another save.
    applyPersonaSnapshot(settings.power_user, {});
    assert.deepEqual(settings.power_user.personas, {});
    assert.deepEqual(buildSettingsPatchSaveRequest(prepareSettingsSavePayload(settings)).patch.persona_updates, {});
});


test('partial Persona saves retain only failed edits for the next save', async () => {
    globalThis.window = { __TAURI_RUNNING__: true };
    const settings = { setting: 'before', power_user: { personas: { 'gone.png': 'Before', 'kept.png': 'Before' } } };
    captureSettingsSaveBaseline(settings, revision);
    settings.setting = 'after';
    settings.power_user.personas['gone.png'] = 'Unsaved';
    settings.power_user.personas['kept.png'] = 'Saved';
    globalThis.fetch = async () => Response.json({
        ...revision, result: 'partial', mode: 'patch', persona_errors: { 'gone.png': 'Persona no longer exists' },
    });
    const result = await trySaveSettingsDelta(prepareSettingsSavePayload(settings), {});
    assert.equal(result.personaErrors['gone.png'], 'Persona no longer exists');
    const retry = buildSettingsPatchSaveRequest(prepareSettingsSavePayload(settings)).patch;
    assert.deepEqual(retry.ops, []);
    assert.deepEqual(retry.persona_updates, { 'gone.png': { name: 'Unsaved' } });
});
