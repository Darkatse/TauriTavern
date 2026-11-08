import { expect, onTestFinished, test } from '@rstest/core';
import { captureBrowserErrors, createFrontend } from '../helpers/frontend.mjs';
import { HOSTS } from '../helpers/host-identity.mjs';

async function openSession(options) {
    const frontend = await createFrontend(options);
    onTestFinished(frontend.close);
    const { window, load, startHost } = frontend;
    const { errors, failed } = captureBrowserErrors(window);
    await startHost();
    const main = (await load('script.js'));
    const { eventSource, event_types } = (await load('scripts/events.js'));
    const ready = new Promise(resolve => eventSource.once(event_types.APP_READY, resolve));
    const extensionsLoaded = new Promise(resolve => eventSource.once(event_types.EXTENSION_SETTINGS_LOADED, resolve));
    const saved = new Promise(resolve => eventSource.once(event_types.SETTINGS_UPDATED, resolve));
    window.jQuery.holdReady(false);
    await Promise.race([Promise.all([ready, saved, extensionsLoaded]), failed]);
    expect(window.document.getElementById('extensions_details').classList.contains('warning')).toBe(false);
    expect(window.webHostFixtureActivated).toBe(true);
    return { ...frontend, main, errors, failed };
}

for (const identity of [HOSTS.windows, HOSTS.android]) {
    // Boots the application twice, so it gets twice the default budget.
    test(`Web Host preserves ${identity.kind} session edits and replayed replies across reload`, async () => {
        const first = await openSession({ identity });
        const { window, fixture, main, load } = first;
        const invoke = window.__TAURI__.core.invoke;
        (await load('scripts/extensions.js')).extension_settings.webHostSession = { enabled: true };
        expect(await main.saveSettings()).toBe(true);
        await invoke('update_tauritavern_settings', { dto: { dynamic_theme: { day_theme: 'Dark Lite' } } });
        await invoke('update_tauritavern_settings', { dto: { dynamic_theme: { night_theme: 'Dark Lite' } } });
        const { extension, mcp } = window.__TAURITAVERN__.api;
        await extension.store.setJson({ namespace: 'webHostSession', key: 'note', value: { kept: true } });
        await mcp.servers.create({ displayName: 'Session tools', endpoint: 'https://tools.invalid/mcp' });
        await window.fetch('/api/themes/save', {
            method: 'POST', headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ name: 'Session theme', blur_strength: 17 }),
        });
        main.chat_metadata.variables = { score: 7 };
        await main.saveMetadata();
        const transport = (await load('scripts/tauri/chat/transport.js'));
        await expect(transport.saveCharacterChatMetadata({
            avatarUrl: 'Review.png', fileName: 'review-chat', chatMetadata: { integrity: 'stale' },
        })).rejects.toSatisfy(error => error.code === 'integrity');
        const card = JSON.parse(JSON.parse(fixture.backend.handle({ method: 'POST', url: '/api/characters/get', headers: {}, body: '{}' }).body).json_data);
        card.data.extensions.tavern_helper = { scripts: [{ id: 'script', content: 'console.log(1)' }], variables: { rank: 2 } };
        window.jQuery('#character_json_data').val(JSON.stringify(card));
        await Promise.race([main.createOrEditCharacter(), first.failed]);
        await (await load('scripts/extensions.js')).writeExtensionField(0, 'merged', true);
        const quickReply = window.quickReplyApi.getSetByName('Review QR');
        quickReply.qrList[0].message = 'A saved quick reply';
        await quickReply.save();
        await Promise.race([window.jQuery('#api_button_openai').triggerHandler('click'), first.failed]);
        expect(main.online_status).not.toBe('no_connection');
        window.jQuery('#send_textarea').val('Continue the story');
        await Promise.race([main.sendTextareaMessage(), first.failed]);
        await main.waitForGenerationIdle();
        expect(main.chat.some(message => message.is_user && message.mes === 'Continue the story')).toBeTruthy();
        expect(main.chat.at(-1).mes).toBe('A replayed reply.');

        const saved = fixture.chatFiles.get('review-chat');
        const corrupted = '{truncated header\n' + saved.split('\n').slice(1).join('\n');
        fixture.chatFiles.set('review-chat', corrupted);
        const repair = { avatarUrl: 'Review.png', fileName: 'review-chat', payload: saved.split('\n').map(line => JSON.parse(line)) };
        await expect(transport.saveCharacterChatPayload(repair)).rejects.toThrow(/400.*integrity/);
        expect(fixture.chatFiles.get('review-chat')).toBe(corrupted);
        await transport.saveCharacterChatPayload({ ...repair, force: true });
        const worldInfo = (await load('scripts/world-info.js'));
        const lore = structuredClone(await worldInfo.loadWorldInfo('Review lore'));
        lore.entries[0].content = 'Review lore edited.';
        await Promise.race([worldInfo.saveWorldInfo('Review lore', lore, true), first.failed]);
        await Promise.race([first.host.exit(), first.failed]);
        expect(first.errors).toEqual([]);
        first.close();

        const second = await openSession({ identity, fixture });
        expect(second.main.chat.at(-1).mes).toBe('A replayed reply.');
        expect(second.main.chat_metadata.variables.score).toBe(7);
        const settings = JSON.parse(JSON.parse(fixture.backend.handle({ method: 'POST', url: '/api/settings/get', headers: {}, body: '{}' }).body).settings);
        expect(settings.extension_settings.webHostSession).toEqual({ enabled: true });
        const stored = await second.window.__TAURI__.core.invoke('get_tauritavern_settings');
        expect(stored.dynamic_theme.day_theme).toBe('Dark Lite');
        expect(stored.dynamic_theme.night_theme).toBe('Dark Lite');
        const api = second.window.__TAURITAVERN__.api;
        expect(await api.extension.store.getJson({ namespace: 'webHostSession', key: 'note' })).toEqual({ kept: true });
        expect((await api.mcp.servers.list()).servers.map(server => server.displayName)).toContain('Session tools');
        const response = JSON.parse(fixture.backend.handle({ method: 'POST', url: '/api/settings/get', headers: {}, body: '{}' }).body);
        expect(response.themes.find(theme => theme.name === 'Session theme').blur_strength).toBe(17);
        const reloadedCard = JSON.parse(fixture.backend.handle({ method: 'POST', url: '/api/characters/get', headers: {}, body: '{}' }).body);
        expect(reloadedCard.data.extensions.tavern_helper).toEqual({ scripts: [{ id: 'script', content: 'console.log(1)' }], variables: { rank: 2 } });
        expect(reloadedCard.data.extensions.merged).toBe(true);
        expect(second.window.quickReplyApi.getSetByName('Review QR').qrList[0].message).toBe('A saved quick reply');
        const reloadedLore = JSON.parse(fixture.backend.handle({ method: 'POST', url: '/api/worldinfo/get', headers: {}, body: '{"name":"Review lore"}' }).body);
        expect(reloadedLore.entries[0].content).toBe('Review lore edited.');
        expect(second.errors).toEqual([]);
    }, 30_000);
}

test('Web Host stream reads replay an unacknowledged cursor and cancellation releases a blocked read', async () => {
    const { window, fixture, close } = await createFrontend();
    onTestFinished(close);
    const handle = fixture.backend.handle.bind(fixture.backend);
    const cancelled = Promise.withResolvers();
    let producer;
    fixture.backend.handle = request => request.url === '/api/backends/chat-completions/generate'
        ? { status: 200, headers: { 'content-type': 'text/event-stream' }, body: new ReadableStream({
            start(controller) { producer = controller; },
            cancel() { cancelled.resolve(); },
        }) }
        : handle(request);
    const invoke = window.__TAURI__.core.invoke;
    await invoke('start_chat_completion_stream', { streamId: 'replay', dto: {} });
    const first = invoke('read_chat_completion_stream', { streamId: 'replay', afterSeq: 0 });
    producer.enqueue(new TextEncoder().encode('data: {"choices":[]}\n\n'));
    const batch = await first;
    expect(batch.events[0].seq).toBe(1);
    expect(batch.events[0].data).toBe('{"choices":[]}');
    expect(await invoke('read_chat_completion_stream', { streamId: 'replay', afterSeq: 0 })).toEqual(batch);
    const blocked = invoke('read_chat_completion_stream', { streamId: 'replay', afterSeq: 1 });
    await invoke('close_chat_completion_stream', { streamId: 'replay' });
    expect((await blocked).status).toBe('cancelled');
    await cancelled.promise;
});
