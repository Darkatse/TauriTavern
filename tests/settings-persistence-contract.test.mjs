import test from 'node:test';
import assert from 'node:assert/strict';
import {
    applyPersonaSnapshot,
    captureSettingsSaveState,
    saveSettingsSnapshot,
} from '../src/scripts/tauri/setting/settings-persistence.js';

const revision = { token: 'loaded' };
const originalFetch = globalThis.fetch;
const originalWindow = globalThis.window;

test.beforeEach(() => { globalThis.window = { __TAURI_RUNNING__: true }; });
test.afterEach(() => {
    globalThis.fetch = originalFetch;
    if (originalWindow === undefined) delete globalThis.window;
    else globalThis.window = originalWindow;
});

test('edits made during a save belong to the next snapshot and revision', async () => {
    const settings = { extension_settings: { history: ['before'] } };
    captureSettingsSaveState(settings, revision);
    const next = { token: 'saved' };
    const acknowledgement = Promise.withResolvers();
    const requests = [];
    globalThis.fetch = (_url, init) => {
        requests.push({
            body: JSON.parse(init.body),
            revision: JSON.parse(init.headers.get('X-TauriTavern-Settings-Revision')),
        });
        return requests.length === 1
            ? acknowledgement.promise
            : Promise.resolve(Response.json({ tauritavern_settings_revision: next }));
    };

    const saving = saveSettingsSnapshot(settings, {});
    settings.extension_settings.history.push('during');
    acknowledgement.resolve(Response.json({ tauritavern_settings_revision: next }));
    await saving;
    await saveSettingsSnapshot(settings, {});

    assert.deepEqual(requests[0].body.extension_settings.history, ['before']);
    assert.deepEqual(requests[1].body.extension_settings.history, ['before', 'during']);
    assert.deepEqual(requests.map(request => request.revision), [revision, next]);
});

test('Persona refreshes preserve pending edits and partial saves retry only failed cards', async () => {
    const settings = { power_user: { personas: { local: 'Before', remote: 'Remote', failed: 'Before' } } };
    captureSettingsSaveState(settings, revision);
    settings.power_user.personas.local = 'Edited';
    settings.power_user.personas.failed = 'Retry';
    applyPersonaSnapshot(settings.power_user, {
        local: { name: 'Before' }, remote: { name: 'Synced' }, failed: { name: 'Before' },
    });
    assert.equal(settings.power_user.personas.local, 'Edited');
    assert.equal(settings.power_user.personas.remote, 'Synced');

    const requests = [];
    globalThis.fetch = async (_url, init) => {
        requests.push(JSON.parse(init.body));
        if (requests.length === 1) {
            applyPersonaSnapshot(settings.power_user, {
                local: { name: 'Edited' }, failed: { name: 'Before' }, arrived: { name: 'New' },
            });
            return Response.json({ tauritavern_settings_revision: revision, persona_errors: { failed: 'Unavailable' } });
        }
        return Response.json({ tauritavern_settings_revision: revision });
    };
    const result = await saveSettingsSnapshot(settings, {});
    assert.equal(result.personaErrors.failed, 'Unavailable');
    await saveSettingsSnapshot(settings, {});
    assert.deepEqual(requests[0].power_user.personas, { local: 'Edited', failed: 'Retry' });
    assert.deepEqual(requests[1].power_user.personas, { failed: 'Retry' });
    applyPersonaSnapshot(settings.power_user, {});
    assert.deepEqual(settings.power_user.personas, {});
});
