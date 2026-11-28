import { expect, onTestFinished, test } from '@rstest/core';
import { createFrontend, createPaginationWaiter } from '../helpers/frontend.mjs';

// Commit frames are UTF-8 bytes; a character may span two frames.
const decodeFrames = frames => {
    const decoder = new TextDecoder();
    return frames.map(frame => decoder.decode(frame, { stream: true })).join('') + decoder.decode();
};

test('deleting characters keeps a shared lorebook until its last user is deleted', async () => {
    const { window, load, startHost, initializeChatRendering, close } = await createFrontend();
    onTestFinished(close);
    await startHost();
    const main = await initializeChatRendering();
    const worldInfo = (await load('scripts/world-info.js'));
    main.setAnimationDuration(0);
    (await load('scripts/tauri/setting/settings-persistence.js'))
        .captureSettingsSaveState({}, { token: 'loaded' });

    let characters = ['Alice', 'Bob'].map(name => ({
        name, avatar: `${name}.png`, chat: 'chat', shallow: false,
        data: { extensions: { world: 'Shared' }, tags: [], creator_notes: '', creator: '' },
    }));
    const worlds = new Set(['Shared']);
    // An empty refresh must not make the surviving character's book appear unused.
    let listReads = 0;
    const notices = [];
    window.toastr.success = message => notices.push(message);
    window.fetch = async (url, options) => {
        const body = options?.body ? JSON.parse(options.body) : undefined;
        switch (url) {
            case '/api/characters/delete':
                characters = characters.filter(character => character.avatar !== body.avatar_url);
                return window.Response.json({ ok: true });
            case '/api/characters/all':
                return window.Response.json(++listReads === 1 ? [] : characters);
            case '/api/groups/all': return window.Response.json([]);
            case '/api/settings/get': return window.Response.json({ world_names: [...worlds] });
            case '/api/settings/save': return window.Response.json({ tauritavern_settings_revision: { token: 'saved' } });
            case '/api/worldinfo/delete':
                worlds.delete(body.name);
                return window.Response.json({ ok: true });
            default: throw new Error(`Unexpected request: ${url}`);
        }
    };
    await worldInfo.updateWorldInfoList();
    worldInfo.world_info.charLore = [{ name: 'Bob', extraBooks: ['Shared'] }];
    main.characters.push(...characters);
    main.setCharacterId(0);

    expect(await main.deleteCharacter('Alice.png', { deleteChats: false })).toBe(true);
    expect(worlds.has('Shared')).toBe(true);
    await main.getCharacters();
    main.setCharacterId(0);
    expect(await main.deleteCharacter('Bob.png', { deleteChats: false })).toBe(true);
    expect(worlds.has('Shared')).toBe(false);
    expect(notices).toEqual(['Also deleted lorebook "Shared"']);
});

test('editing after a backend lorebook update preserves the new document', async () => {
    const { window, load, startHost, close } = await createFrontend();
    onTestFinished(close);
    await startHost();
    await load('script.js');
    const worldInfo = (await load('scripts/world-info.js'));
    const $ = window.jQuery;
    const waitForPagination = createPaginationWaiter(window);

    const entry = (uid, content) => ({ ...structuredClone(worldInfo.newWorldInfoEntryTemplate), uid, content });
    const oldDocument = { entries: { 0: entry(0, 'old content') } };
    const newDocument = { entries: { 0: entry(0, 'new content'), 1: entry(1, 'new entry') } };
    let saved;
    window.fetch = async (url, init) => {
        if (url === '/api/settings/get') return new window.Response(JSON.stringify({ world_names: ['Book'] }));
        if (url === '/api/worldinfo/get') return new window.Response(JSON.stringify(newDocument));
        if (url === '/api/worldinfo/edit') {
            saved = JSON.parse(init.body).data;
            return new window.Response('{}');
        }
        throw new Error(`Unexpected request: ${url}`);
    };
    await worldInfo.updateWorldInfoList();
    worldInfo.initWorldInfo();
    $('#world_editor_select').val('0');
    worldInfo.worldInfoCache.set('Book', oldDocument);
    await waitForPagination(() => worldInfo.showWorldEditor('Book'));

    await worldInfo.saveWorldInfo('Book', oldDocument);
    await waitForPagination(() => worldInfo.refreshWorldInfoAfterExternalWrite('Book'));
    await worldInfo.flushWorldInfoSaves();
    expect(saved).toBeUndefined();
    const order = window.document.querySelector('#world_popup_entries_list input[name="order"]');
    order.value = '101';
    await $(order).triggerHandler('input');
    await worldInfo.flushWorldInfoSaves();
    expect(saved.entries[0].content).toBe('new content');
    expect(saved.entries[1].content).toBe('new entry');
    expect(saved.entries[0].order).toBe(101);
});

test('immediate and debounced world info saves keep their snapshots and later edits', async () => {
    const { window, load, startHost, close } = await createFrontend();
    onTestFinished(close);
    await startHost();
    await load('script.js');
    const worldInfo = (await load('scripts/world-info.js'));
    const { eventSource, event_types } = (await load('scripts/events.js'));
    const started = Promise.withResolvers();
    const release = Promise.withResolvers();
    const saved = [];
    const events = [];
    eventSource.on(event_types.WORLDINFO_UPDATED, async name => {
        events.push(name);
        if (name === 'Book' && events.length === 1) {
            await worldInfo.saveWorldInfo('Derived', { entries: {} }, true);
        }
    });
    window.fetch = async (url, init) => {
        expect(url).toBe('/api/worldinfo/edit');
        saved.push(JSON.parse(init.body));
        if (saved.length === 1) {
            started.resolve();
            await release.promise;
        }
        return new window.Response('{}');
    };

    const data = { entries: { 0: { content: 'first' } } };
    const immediate = worldInfo.saveWorldInfo('Book', data, true);
    await started.promise;
    expect(events).toEqual([]);
    data.entries[0].content = 'later';
    await worldInfo.saveWorldInfo('Book', data);
    const flushed = worldInfo.flushWorldInfoSaves();
    release.resolve();
    await Promise.all([immediate, flushed]);
    expect(saved.filter(item => item.name === 'Book')
        .map(item => item.data.entries[0].content)).toEqual(['first', 'later']);
    expect(saved.some(item => item.name === 'Derived')).toBeTruthy();
    expect(events.filter(name => name === 'Book').length).toBe(2);
});

test('world info failures stop dependent reads and leave unrelated saves and retries usable', async () => {
    const { window, load, startHost, close } = await createFrontend();
    onTestFinished(close);
    await startHost();
    await load('script.js');
    const worldInfo = (await load('scripts/world-info.js'));
    const saved = new Map();
    let writable = false;
    let readable = false;
    window.fetch = async (url, init) => {
        const { name, data } = JSON.parse(init.body);
        if (url === '/api/worldinfo/edit') {
            if (name === 'Broken' && !writable) return new window.Response('disk error', { status: 500 });
            saved.set(name, data);
            return new window.Response('{}');
        }
        expect(url).toBe('/api/worldinfo/get');
        return readable
            ? new window.Response('{"entries":{"0":{"uid":0,"content":"recovered read"}}}')
            : new window.Response('invalid JSON', { status: 500 });
    };

    await worldInfo.saveWorldInfo('Broken', { entries: { 0: { uid: 0, content: 'pending save' } } });
    await worldInfo.saveWorldInfo('Good', { entries: { 0: { uid: 0, content: 'good' } } });
    await expect(worldInfo.flushWorldInfoSaves()).rejects.toThrow(/Broken/);
    expect(saved.get('Good').entries[0].content).toBe('good');
    expect(saved.has('Broken')).toBe(false);

    worldInfo.selected_world_info.push('Good');
    const available = await worldInfo.getSortedEntries();
    expect(available.find(entry => entry.world === 'Good').content).toBe('good');
    worldInfo.selected_world_info.push('Broken');
    await expect(worldInfo.getSortedEntries()).rejects.toThrow(/Broken/);
    writable = true;
    const afterSaveRetry = await worldInfo.getSortedEntries();
    expect(afterSaveRetry.find(entry => entry.world === 'Broken').content).toBe('pending save');
    expect(saved.get('Broken').entries[0].content).toBe('pending save');

    worldInfo.selected_world_info.push('Unreadable');
    await expect(worldInfo.getSortedEntries()).rejects.toThrow(/Unreadable/);
    readable = true;
    const afterReadRetry = await worldInfo.getSortedEntries();
    expect(afterReadRetry.find(entry => entry.world === 'Unreadable').content).toBe('recovered read');
});

test('performance HUD keeps world info byte commits intact', async () => {
    const { window, load, close } = await createFrontend();
    let hud;
    onTestFinished(async () => {
        hud?.disable();
        close();
    });
    const { createInvokeService } = (await load('tauri/main/services/invokes/invoke-service.js'));
    const { createRouteRegistry } = (await load('tauri/main/router.js'));
    const { jsonResponse } = (await load('tauri/main/http-utils.js'));
    const { registerWorldInfoRoutes } = (await load('tauri/main/routes/worldinfo-routes.js'));
    const { installPerfHud } = (await load('tauri/main/perf/perf-hud.js'));
    const frames = [];
    let saved;
    const service = createInvokeService({
        policies: {},
        async invoke(command, args, options) {
            switch (command) {
                case 'begin_world_info_commit': return { sessionId: 'world', maxFrameBytes: 11 };
                case 'append_world_info_commit_chunk': {
                    frames.push(new Uint8Array(args));
                    return Number(options?.headers?.offset) + args.byteLength;
                }
                case 'finish_world_info_commit': saved = decodeFrames(frames); return;
                default: throw new Error(`Unexpected command ${command}`);
            }
        },
    });
    const router = createRouteRegistry();
    registerWorldInfoRoutes(router, service, { jsonResponse });
    hud = installPerfHud({ context: service });
    hud.enable();
    const body = '{"name":"Book","data":{"entries":{},"text":"你好 👋"}}';
    const response = await router.handle({ method: 'POST', path: '/api/worldinfo/edit', body });
    expect(response.status).toBe(200);
    expect(saved).toBe(body);
});
