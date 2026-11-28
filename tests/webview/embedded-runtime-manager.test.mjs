import { afterEach, expect, rs, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const { createManagedIframeSlot } = await importFrontend('tauri/main/adapters/embedded-runtime/managed-iframe-slot.js');
const { createEmbeddedRuntimeManager } = await importFrontend('tauri/main/services/embedded-runtime/embedded-runtime-manager.js');

const cleanups = [];
afterEach(() => {
    for (const cleanup of cleanups.splice(0)) cleanup();
    rs.useRealTimers();
});

function createRecordingSlot({ id, kind, element, visibilityMode = 'manual', initialVisible = false, priority = 0, weight = 1, iframeCount = 1 }) {
    const calls = [];
    return {
        slot: {
            id,
            kind,
            element,
            visibilityMode,
            initialVisible,
            priority,
            weight,
            iframeCount,
            hydrate(reason) {
                calls.push({ type: 'hydrate', id, reason });
            },
            dehydrate(reason) {
                calls.push({ type: 'dehydrate', id, reason });
            },
            dispose() {
                calls.push({ type: 'dispose', id });
            },
        },
        calls,
    };
}

test('EmbeddedRuntimeManager enforces budgets and chooses a stable active set', async () => {
    let ts = 0;
    const now = () => ts;

    const manager = createEmbeddedRuntimeManager({
        now,
        profile: {
            name: 'test',
            maxActiveWeight: 15,
            maxActiveIframes: 2,
            maxActiveSlots: 2,
            maxSoftParkedIframes: 0,
            softParkTtlMs: 0,
            parkWhenHiddenKinds: ['k'],
            rootMargin: '0px',
            threshold: 0,
        },
    });

    const el1 = document.createElement('div');
    const el2 = document.createElement('div');
    const el3 = document.createElement('div');
    document.body.append(el1, el2, el3);

    const s1 = createRecordingSlot({ id: 's1', kind: 'k', element: el1, initialVisible: true, weight: 10 });
    const s2 = createRecordingSlot({ id: 's2', kind: 'k', element: el2, initialVisible: true, weight: 10 });
    const s3 = createRecordingSlot({ id: 's3', kind: 'k', element: el3, initialVisible: false, weight: 1 });

    manager.register(s1.slot);
    manager.register(s2.slot);
    manager.register(s3.slot);

    manager.reconcile();

    // Budget picks one visible slot; tie breaks by id.
    expect(s1.calls[0]).toEqual({ type: 'hydrate', id: 's1', reason: 'manual' });
    expect(s2.calls[0]).toEqual({ type: 'dehydrate', id: 's2', reason: 'budget' });
    expect(s3.calls[0]).toEqual({ type: 'dehydrate', id: 's3', reason: 'visibility' });

    const snap = manager.getPerfSnapshot();
    expect(snap.active).toBe(1);
    expect(snap.parked).toBe(2);
    expect(snap.activeWeight).toBe(10);
    expect(snap.activeIframes).toBe(1);
});

test('EmbeddedRuntimeManager invalidate() forces a re-hydrate for active candidates', async () => {
    let ts = 0;
    const now = () => (ts += 1);

    const manager = createEmbeddedRuntimeManager({
        now,
        profile: {
            name: 'test',
            maxActiveWeight: 100,
            maxActiveIframes: 10,
            maxActiveSlots: 10,
            maxSoftParkedIframes: 0,
            softParkTtlMs: 0,
            parkWhenHiddenKinds: [],
            rootMargin: '0px',
            threshold: 0,
        },
    });

    const slot = createRecordingSlot({ id: 's', kind: 'keep', element: document.createElement('div'), initialVisible: true });
    document.body.append(slot.slot.element);
    manager.register(slot.slot);
    manager.reconcile();

    const hydratesBefore = slot.calls.filter((c) => c.type === 'hydrate').length;
    manager.invalidate('s');
    manager.reconcile();
    const hydratesAfter = slot.calls.filter((c) => c.type === 'hydrate').length;

    expect(hydratesAfter).toBe(hydratesBefore + 1);
    expect(() => manager.invalidate('missing')).toThrow(/slot not found/);
});

function setupManagedIframes() {
    rs.useFakeTimers({ toFake: ['requestAnimationFrame', 'cancelAnimationFrame'] });
    const manager = createEmbeddedRuntimeManager({
        profile: {
            name: 'test', maxActiveWeight: 10, maxActiveIframes: 1, maxActiveSlots: 1,
            maxSoftParkedIframes: 0, softParkTtlMs: 0,
            parkWhenHiddenKinds: ['k'], rootMargin: '0px', threshold: 0,
        },
    });
    const ids = [];
    cleanups.push(() => {
        for (const id of ids) manager.unregister(id);
    });
    const create = (id, attributes, initialVisible) => {
        const host = document.createElement('div');
        const iframe = document.createElement('iframe');
        Object.assign(iframe, attributes);
        host.append(iframe);
        document.body.append(host);
        const slot = createManagedIframeSlot({
            id, kind: 'k', host, maxSoftParkedIframes: 0, softParkTtlMs: 0,
            onSourceSettled: () => manager.invalidate(id),
        });
        manager.register({ ...slot, visibilityMode: 'manual', initialVisible });
        ids.push(id);
        return { host, iframe };
    };
    return { manager, create };
}

// Advance a task boundary to drain the explicitly released source promises.
const settleReads = () => new Promise(resolve => setTimeout(resolve, 0));
// Bodies arrive in memory: reading a real Response takes an engine-dependent number of tasks.
const fetched = html => ({ ok: true, blob: async () => new Blob([html], { type: 'text/html' }) });

function deferSourceRead() {
    const read = Promise.withResolvers();
    rs.spyOn(window, 'fetch').mockImplementation(() => read.promise);
    return read;
}

for (const visible of [false, true]) {
    test(`EmbeddedRuntimeManager uses current visibility after capture: visible=${visible}`, async () => {
        const { manager, create } = setupManagedIframes();
        const read = deferSourceRead();
        const { host, iframe } = create('a', { src: 'blob:pending' }, true);
        manager.reconcile();
        manager.setVisible('a', false);
        manager.reconcile();
        expect(host.querySelector('iframe'), 'capture must precede eviction').toBe(iframe);
        expect(manager.getPerfSnapshot().activeIframes).toBe(1);

        manager.setVisible('a', visible);
        manager.reconcile();
        read.resolve(fetched('<p>original</p>'));
        await settleReads();
        expect(host.querySelector('iframe'), 'source completion must not mutate DOM').toBe(iframe);
        rs.advanceTimersToNextFrame();
        expect(host.querySelector('iframe')).toBe(visible ? iframe : null);
        expect(manager.getPerfSnapshot().activeIframes).toBe(visible ? 1 : 0);
    });
}

for (const visible of [false, true]) {
    test(`EmbeddedRuntimeManager recovers an externally removed page only if still wanted: visible=${visible}`, async () => {
        const { manager, create } = setupManagedIframes();
        const read = deferSourceRead();
        const { host, iframe } = create('a', { src: 'blob:pending' }, true);
        manager.reconcile();
        iframe.remove();
        manager.invalidate('a');
        manager.reconcile();
        expect(manager.getPerfSnapshot().activeIframes, 'pending recovery is not active').toBe(0);

        manager.setVisible('a', visible);
        manager.reconcile();
        read.resolve(fetched('<p>original</p>'));
        await settleReads();
        expect(host.querySelector('iframe')).toBeNull();
        rs.advanceTimersToNextFrame();
        expect(host.querySelector('iframe')).toBe(visible ? iframe : null);
        expect(manager.getPerfSnapshot().activeIframes).toBe(visible ? 1 : 0);
    });
}

test('EmbeddedRuntimeManager cannot resurrect a disposed slot when its source read completes', async () => {
    const { manager, create } = setupManagedIframes();
    const read = deferSourceRead();
    const { host } = create('a', { src: 'blob:pending' }, true);
    manager.reconcile();
    manager.unregister('a');
    read.resolve(fetched('<p>original</p>'));
    await settleReads();
    rs.advanceTimersToNextFrame();
    expect(host.children.length).toBe(0);
    expect(manager.getPerfSnapshot().activeIframes).toBe(0);
});

function prepareCapacityHandoff() {
    const context = setupManagedIframes();
    const { manager, create } = context;
    const read = deferSourceRead();
    // Register the replacement first: grant order must not depend on Map order.
    const b = create('b', { srcdoc: '<p>replacement</p>' }, false);
    const a = create('a', { src: 'blob:pending' }, true);
    manager.reconcile();
    manager.setVisible('a', false);
    manager.setVisible('b', true);
    manager.reconcile();
    return { ...context, read, a, b };
}

test('EmbeddedRuntimeManager grants replacement capacity only after the live page can be evicted', async () => {
    const { manager, read, a, b } = prepareCapacityHandoff();
    expect(a.host.querySelector('iframe')).toBe(a.iframe);
    expect(b.host.querySelector('iframe')).toBeNull();
    expect(manager.getPerfSnapshot().activeIframes).toBe(1);

    read.resolve(fetched('<p>source</p>'));
    await settleReads();
    rs.advanceTimersToNextFrame();
    expect(a.host.querySelector('iframe')).toBeNull();
    expect(b.host.querySelector('iframe')).toBe(b.iframe);
    expect(manager.getPerfSnapshot().activeIframes).toBe(1);
});

for (const [failure, finishRead] of [
    ['rejected fetch', read => read.reject(new Error('source unavailable'))],
    ['HTTP error', read => read.resolve({ ok: false, status: 404 })],
]) {
    test(`EmbeddedRuntimeManager counts an unrecoverable live page until disposal: ${failure}`, async () => {
        const { manager, read, a, b } = prepareCapacityHandoff();
        rs.spyOn(console, 'warn').mockImplementation(() => {});
        finishRead(read);
        await settleReads();
        rs.advanceTimersToNextFrame();
        expect(a.host.querySelector('iframe')).toBe(a.iframe);
        expect(b.host.querySelector('iframe')).toBeNull();
        expect(manager.getPerfSnapshot().activeIframes).toBe(1);

        manager.unregister('a');
        rs.advanceTimersToNextFrame();
        expect(a.host.querySelector('iframe')).toBeNull();
        expect(b.host.querySelector('iframe')).toBe(b.iframe);
        expect(manager.getPerfSnapshot().activeIframes).toBe(1);
    });
}

test('EmbeddedRuntimeManager keeps a healthy resident when a preferred missing page is pending or failed', async () => {
    const { manager, create } = setupManagedIframes();
    const read = deferSourceRead();
    rs.spyOn(console, 'warn').mockImplementation(() => {});
    const healthy = create('b', { srcdoc: '<input value="initial">' }, true);
    let loads = 0;
    const loaded = new Promise(resolve => healthy.iframe.addEventListener('load', () => resolve(++loads)));
    await loaded;
    const healthyDocument = healthy.iframe.contentDocument;
    healthyDocument.querySelector('input').value = 'kept';
    const expectHealthyResident = () => {
        expect(healthy.host.querySelector('iframe')).toBe(healthy.iframe);
        expect(healthy.iframe.contentDocument).toBe(healthyDocument);
        expect(healthyDocument.querySelector('input').value).toBe('kept');
        expect(loads).toBe(1);
        expect(manager.getPerfSnapshot().activeIframes).toBe(1);
    };
    manager.reconcile();
    const missing = create('a', { src: 'blob:pending' }, false);
    missing.iframe.remove();
    manager.invalidate('a');
    manager.setVisible('a', true);
    manager.touch('a');
    manager.reconcile();
    expectHealthyResident();
    expect(missing.host.querySelector('iframe')).toBeNull();

    read.reject(new Error('source unavailable'));
    await settleReads();
    rs.advanceTimersToNextFrame();
    manager.reconcile();
    expectHealthyResident();
    expect(missing.host.querySelector('iframe')).toBeNull();
    const placeholder = missing.host.querySelector('.tt-runtime-placeholder');
    expect(placeholder.textContent).toMatch(/Cannot restore this page locally/);
    expect(placeholder.tabIndex).toBe(-1);
});
