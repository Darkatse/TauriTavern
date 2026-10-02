import assert from 'node:assert/strict';
import test from 'node:test';
import { createBrowserRuntime } from './runtime.mjs';

test('immediate and debounced world info saves keep their snapshots and later edits', async context => {
    const { window, getModule, load, startHost } = createBrowserRuntime();
    context.after(() => window.happyDOM.close());
    await startHost();
    await load('script.js');
    const worldInfo = getModule('scripts/world-info.js').namespace;
    const { eventSource, event_types } = getModule('scripts/events.js').namespace;
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
        assert.equal(url, '/api/worldinfo/edit');
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
    assert.deepEqual(events, []);
    data.entries[0].content = 'later';
    await worldInfo.saveWorldInfo('Book', data);
    const flushed = worldInfo.flushWorldInfoSaves();
    release.resolve();
    await Promise.all([immediate, flushed]);
    assert.deepEqual(saved.filter(item => item.name === 'Book')
        .map(item => item.data.entries[0].content), ['first', 'later']);
    assert.ok(saved.some(item => item.name === 'Derived'));
    assert.equal(events.filter(name => name === 'Book').length, 2);
});

test('world info failures stop dependent reads and leave unrelated saves and retries usable', async context => {
    const { window, getModule, load, startHost } = createBrowserRuntime();
    context.after(() => window.happyDOM.close());
    await startHost();
    await load('script.js');
    const worldInfo = getModule('scripts/world-info.js').namespace;
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
        assert.equal(url, '/api/worldinfo/get');
        return readable
            ? new window.Response('{"entries":{"0":{"uid":0,"content":"recovered read"}}}')
            : new window.Response('invalid JSON', { status: 500 });
    };

    await worldInfo.saveWorldInfo('Broken', { entries: { 0: { uid: 0, content: 'pending save' } } });
    await worldInfo.saveWorldInfo('Good', { entries: { 0: { uid: 0, content: 'good' } } });
    await assert.rejects(worldInfo.flushWorldInfoSaves(), /Broken/);
    assert.equal(saved.get('Good').entries[0].content, 'good');
    assert.equal(saved.has('Broken'), false);

    worldInfo.selected_world_info.push('Good');
    const available = await worldInfo.getSortedEntries();
    assert.equal(available.find(entry => entry.world === 'Good').content, 'good');
    worldInfo.selected_world_info.push('Broken');
    await assert.rejects(worldInfo.getSortedEntries(), /Broken/);
    writable = true;
    const afterSaveRetry = await worldInfo.getSortedEntries();
    assert.equal(afterSaveRetry.find(entry => entry.world === 'Broken').content, 'pending save');
    assert.equal(saved.get('Broken').entries[0].content, 'pending save');

    worldInfo.selected_world_info.push('Unreadable');
    await assert.rejects(worldInfo.getSortedEntries(), /Unreadable/);
    readable = true;
    const afterReadRetry = await worldInfo.getSortedEntries();
    assert.equal(afterReadRetry.find(entry => entry.world === 'Unreadable').content, 'recovered read');
});

test('performance HUD keeps world info byte commits intact', async context => {
    const { window, load } = createBrowserRuntime();
    let hud;
    context.after(async () => {
        hud?.disable();
        await window.happyDOM.close();
    });
    const { createInvokeService } = (await load('tauri/main/services/invokes/invoke-service.js')).namespace;
    const { createRouteRegistry } = (await load('tauri/main/router.js')).namespace;
    const { jsonResponse } = (await load('tauri/main/http-utils.js')).namespace;
    const { registerWorldInfoRoutes } = (await load('tauri/main/routes/worldinfo-routes.js')).namespace;
    const { installPerfHud } = (await load('tauri/main/perf/perf-hud.js')).namespace;
    const frames = [];
    let saved;
    const service = createInvokeService({
        policies: {},
        async invoke(command, args, options) {
            switch (command) {
                case 'begin_world_info_commit': return { sessionId: 'world', maxFrameBytes: 11 };
                case 'append_world_info_commit_chunk': {
                    frames.push(Buffer.from(args));
                    return Number(options?.headers?.offset) + args.byteLength;
                }
                case 'finish_world_info_commit': saved = Buffer.concat(frames).toString(); return;
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
    assert.equal(response.status, 200);
    assert.equal(saved, body);
});
