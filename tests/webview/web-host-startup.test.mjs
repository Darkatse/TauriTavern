import { expect, onTestFinished, test } from '@rstest/core';
// Fails when a command the browser host does not map, or an ST request the fixture does not serve,
// breaks startup: add it in src/tauri/web-host/ (docs/CurrentState/WebHost.md).
import { captureBrowserErrors, createFrontend } from '../helpers/frontend.mjs';
import { HOSTS } from '../helpers/host-identity.mjs';

for (const identity of [HOSTS.windows, HOSTS.android]) {
    test(`Web Host starts the ${identity.kind} app and opens its character chat`, async () => {
        const { window, fixture, load, startHost, close } = await createFrontend({ identity });
        const readSettings = () => JSON.parse(JSON.parse(fixture.backend.handle({
            method: 'POST', url: '/api/settings/get', headers: {}, body: '{}',
        }).body).settings);
        // Settings that lost amount_gen keep starting and save its default (#391).
        const legacy = readSettings();
        const original = legacy.power_user;
        delete legacy.amount_gen;
        fixture.backend.handle({ method: 'POST', url: '/api/settings/save', headers: {}, body: JSON.stringify(legacy) });
        const { errors, failed } = captureBrowserErrors(window);
        try {
            await startHost();
            await load('script.js');
            const { eventSource, event_types } = (await load('scripts/events.js'));
            const ready = new Promise(resolve => eventSource.once(event_types.APP_READY, resolve));
            const extensionsLoaded = new Promise(resolve => eventSource.once(event_types.EXTENSION_SETTINGS_LOADED, resolve));
            const settingsSaved = new Promise(resolve => eventSource.once(event_types.SETTINGS_UPDATED, resolve));
            const loreLoaded = new Promise(resolve => eventSource.on(event_types.WORLDINFO_ENTRIES_LOADED, ({ characterLore }) => {
                if (characterLore.length) resolve(characterLore);
            }));
            window.jQuery.holdReady(false);
            const [, lore] = await Promise.race([Promise.all([ready, loreLoaded, settingsSaved, extensionsLoaded]), failed]);
            expect(window.document.querySelector('#chat .mes_text').textContent).toMatch(/A chat loaded through Web Host/);
            expect(window.document.getElementById('extensions_details').classList.contains('warning')).toBe(false);
            expect(window.webHostFixtureActivated).toBe(true);
            expect(lore[0].content).toBe('Review lore loaded.');
            const saved = readSettings();
            expect(saved.amount_gen).toBe(80);
            expect(saved.power_user.personas).toEqual(original.personas);
            expect(saved.power_user.persona_descriptions).toEqual(original.persona_descriptions);
            expect(errors).toEqual([]);
        } finally {
            close();
        }
    });
}

test('Web Host distinguishes a missing chat from an empty chat file', async () => {
    const { window, fixture, load, close } = await createFrontend();
    onTestFinished(close);
    const handle = fixture.backend.handle.bind(fixture.backend);
    let payload = {};
    fixture.backend.handle = request => request.url === '/api/chats/get'
        ? { status: 200, headers: {}, body: JSON.stringify(payload) }
        : handle(request);
    const { loadCharacterChatPayload } = (await load('scripts/tauri/chat/transport.js'));
    const target = { avatarUrl: 'Review.png', fileName: 'missing' };
    expect((await loadCharacterChatPayload({ ...target, allowNotFound: true })).length).toBe(0);
    await expect(loadCharacterChatPayload(target)).rejects.toSatisfy(error => {
        expect(error.NotFound).toMatch(/Review\/missing/);
        return true;
    });
    payload = [];
    expect((await loadCharacterChatPayload(target)).length).toBe(0);
});
