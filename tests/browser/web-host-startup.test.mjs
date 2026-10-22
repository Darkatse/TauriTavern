// Fails when a command the browser host does not map, or an ST request the fixture does not serve,
// breaks startup: add it in src/tauri/web-host/ (docs/CurrentState/WebHost.md).
import assert from 'node:assert/strict';
import test from 'node:test';
import { captureBrowserErrors, createBrowserRuntime } from './runtime.mjs';
import { HOSTS } from '../helpers/host-identity.mjs';

for (const identity of [HOSTS.windows, HOSTS.android]) {
    test(`Web Host starts the ${identity.kind} app and opens its character chat`, async () => {
        const { window, fixture, load, getModule, startHost } = await createBrowserRuntime({ identity });
        const readSettings = () => JSON.parse(JSON.parse(fixture.backend.handle({
            method: 'POST', url: '/api/settings/get', headers: {}, body: '{}',
        }).body).settings);
        const original = readSettings().power_user;
        const { errors, failed } = captureBrowserErrors(window);
        try {
            await startHost();
            await load('script.js');
            const { eventSource, event_types } = getModule('scripts/events.js').namespace;
            const ready = new Promise(resolve => eventSource.once(event_types.APP_READY, resolve));
            const extensionsLoaded = new Promise(resolve => eventSource.once(event_types.EXTENSION_SETTINGS_LOADED, resolve));
            const settingsSaved = new Promise(resolve => eventSource.once(event_types.SETTINGS_UPDATED, resolve));
            const loreLoaded = new Promise(resolve => eventSource.on(event_types.WORLDINFO_ENTRIES_LOADED, ({ characterLore }) => {
                if (characterLore.length) resolve(characterLore);
            }));
            window.jQuery.holdReady(false);
            const [, lore] = await Promise.race([Promise.all([ready, loreLoaded, settingsSaved, extensionsLoaded]), failed]);
            assert.match(window.document.querySelector('#chat .mes_text').textContent, /A chat loaded through Web Host/);
            assert.equal(window.document.getElementById('extensions_details').classList.contains('warning'), false);
            assert.equal(window.webHostFixtureActivated, true);
            assert.equal(lore[0].content, 'Review lore loaded.');
            const saved = readSettings().power_user;
            assert.deepEqual(saved.personas, original.personas);
            assert.deepEqual(saved.persona_descriptions, original.persona_descriptions);
            assert.deepEqual(errors, []);
        } finally {
            await window.happyDOM.close();
        }
    });
}

test('Web Host distinguishes a missing chat from an empty chat file', async context => {
    const { window, fixture, load } = await createBrowserRuntime();
    context.after(() => window.happyDOM.close());
    const handle = fixture.backend.handle.bind(fixture.backend);
    let payload = {};
    fixture.backend.handle = request => request.url === '/api/chats/get'
        ? { status: 200, headers: {}, body: JSON.stringify(payload) }
        : handle(request);
    const { loadCharacterChatPayload } = (await load('scripts/tauri/chat/transport.js')).namespace;
    const target = { avatarUrl: 'Review.png', fileName: 'missing' };
    assert.equal((await loadCharacterChatPayload({ ...target, allowNotFound: true })).length, 0);
    await assert.rejects(loadCharacterChatPayload(target), error => {
        assert.match(error.NotFound, /Review\/missing/);
        return true;
    });
    payload = [];
    assert.equal((await loadCharacterChatPayload(target)).length, 0);
});
