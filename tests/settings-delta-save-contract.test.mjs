import test from 'node:test';
import assert from 'node:assert/strict';

import {
    buildSettingsPatchSaveRequest,
    captureSettingsSaveBaseline,
    clearSettingsSaveBaseline,
    isSettingsPatchConflictError,
    prepareSettingsSavePayload,
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

test('buildSettingsPatchSaveRequest uses a root set patch when object diffs are too many', () => {
    const next = {};
    for (let index = 0; index < 300; index++) {
        next[`setting_${index}`] = index;
    }

    captureSettingsSaveBaseline({}, revision);

    const request = buildSettingsPatchSaveRequest(prepareSettingsSavePayload(next));

    assert.ok(request);
    assert.deepEqual(request.patch.ops, [{ op: 'set', path: [], value: next }]);
    assert.equal(request.patch.base_hash, revision.settings_hash);
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
