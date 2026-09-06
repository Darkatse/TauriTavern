import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { installFakeDom } from './helpers/fake-dom.mjs';
import { createManagedIframeSlot } from '../src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js';
import { createEmbeddedRuntimeManager } from '../src/tauri/main/services/embedded-runtime/embedded-runtime-manager.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

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
    const dom = installFakeDom();
    try {
        let ts = 0;
        const now = () => ts;

        const { createEmbeddedRuntimeManager } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-manager.js'),
        );

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
        assert.deepEqual(s1.calls[0], { type: 'hydrate', id: 's1', reason: 'manual' });
        assert.deepEqual(s2.calls[0], { type: 'dehydrate', id: 's2', reason: 'budget' });
        assert.deepEqual(s3.calls[0], { type: 'dehydrate', id: 's3', reason: 'visibility' });

        const snap = manager.getPerfSnapshot();
        assert.equal(snap.active, 1);
        assert.equal(snap.parked, 2);
        assert.equal(snap.activeWeight, 10);
        assert.equal(snap.activeIframes, 1);
    } finally {
        dom.cleanup();
    }
});


test('EmbeddedRuntimeManager invalidate() forces a re-hydrate for active candidates', async () => {
    const dom = installFakeDom();
    try {
        let ts = 0;
        const now = () => (ts += 1);

        const { createEmbeddedRuntimeManager } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-manager.js'),
        );

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

        assert.equal(hydratesAfter, hydratesBefore + 1);
        assert.throws(() => manager.invalidate('missing'), /slot not found/);
    } finally {
        dom.cleanup();
    }
});

function setupManagedIframes(t) {
    const nativeQueueMicrotask = globalThis.queueMicrotask;
    const dom = installFakeDom();
    globalThis.queueMicrotask = nativeQueueMicrotask;
    const manager = createEmbeddedRuntimeManager({
        profile: {
            name: 'test', maxActiveWeight: 10, maxActiveIframes: 1, maxActiveSlots: 1,
            maxSoftParkedIframes: 0, softParkTtlMs: 0,
            parkWhenHiddenKinds: ['k'], rootMargin: '0px', threshold: 0,
        },
    });
    const ids = [];
    t.after(() => {
        for (const id of ids) manager.unregister(id);
        dom.cleanup();
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
    return { dom, manager, create };
}

// Advance a task boundary to drain the explicitly released source promises;
// animation-frame delivery stays under the test's control.
const settleReads = () => new Promise(resolve => setImmediate(resolve));

function deferSourceRead(t) {
    const read = Promise.withResolvers();
    t.mock.method(globalThis, 'fetch', () => read.promise);
    return read;
}

for (const visible of [false, true]) {
    test(`EmbeddedRuntimeManager uses current visibility after capture: visible=${visible}`, async (t) => {
        const { dom, manager, create } = setupManagedIframes(t);
        const read = deferSourceRead(t);
        const { host, iframe } = create('a', { src: 'blob:pending' }, true);
        manager.reconcile();
        manager.setVisible('a', false);
        manager.reconcile();
        assert.equal(host.querySelector('iframe'), iframe, 'capture must precede eviction');
        assert.equal(manager.getPerfSnapshot().activeIframes, 1);

        manager.setVisible('a', visible);
        manager.reconcile();
        read.resolve(new Response('<p>original</p>'));
        await settleReads();
        assert.equal(host.querySelector('iframe'), iframe, 'source completion must not mutate DOM');
        dom.flushRaf();
        assert.equal(host.querySelector('iframe'), visible ? iframe : null);
        assert.equal(manager.getPerfSnapshot().activeIframes, visible ? 1 : 0);
    });
}

for (const visible of [false, true]) {
    test(`EmbeddedRuntimeManager recovers an externally removed page only if still wanted: visible=${visible}`, async (t) => {
        const { dom, manager, create } = setupManagedIframes(t);
        const read = deferSourceRead(t);
        const { host, iframe } = create('a', { src: 'blob:pending' }, true);
        manager.reconcile();
        iframe.remove();
        manager.invalidate('a');
        manager.reconcile();
        assert.equal(manager.getPerfSnapshot().activeIframes, 0, 'pending recovery is not active');

        manager.setVisible('a', visible);
        manager.reconcile();
        read.resolve(new Response('<p>original</p>'));
        await settleReads();
        assert.equal(host.querySelector('iframe'), null);
        dom.flushRaf();
        assert.equal(host.querySelector('iframe'), visible ? iframe : null);
        assert.equal(manager.getPerfSnapshot().activeIframes, visible ? 1 : 0);
    });
}

test('EmbeddedRuntimeManager cannot resurrect a disposed slot when its source read completes', async (t) => {
    const { dom, manager, create } = setupManagedIframes(t);
    const read = deferSourceRead(t);
    const { host } = create('a', { src: 'blob:pending' }, true);
    manager.reconcile();
    manager.unregister('a');
    read.resolve(new Response('<p>original</p>'));
    await settleReads();
    dom.flushRaf();
    assert.equal(host.children.length, 0);
    assert.equal(manager.getPerfSnapshot().activeIframes, 0);
});

function prepareCapacityHandoff(t) {
    const context = setupManagedIframes(t);
    const { manager, create } = context;
    const read = deferSourceRead(t);
    // Register the replacement first: grant order must not depend on Map order.
    const b = create('b', { srcdoc: '<p>replacement</p>' }, false);
    const a = create('a', { src: 'blob:pending' }, true);
    manager.reconcile();
    manager.setVisible('a', false);
    manager.setVisible('b', true);
    manager.reconcile();
    return { ...context, read, a, b };
}

test('EmbeddedRuntimeManager grants replacement capacity only after the live page can be evicted', async (t) => {
    const { dom, manager, read, a, b } = prepareCapacityHandoff(t);
    assert.equal(a.host.querySelector('iframe'), a.iframe);
    assert.equal(b.host.querySelector('iframe'), null);
    assert.equal(manager.getPerfSnapshot().activeIframes, 1);

    read.resolve(new Response('<p>source</p>'));
    await settleReads();
    dom.flushRaf();
    assert.equal(a.host.querySelector('iframe'), null);
    assert.equal(b.host.querySelector('iframe'), b.iframe);
    assert.equal(manager.getPerfSnapshot().activeIframes, 1);
});

for (const [failure, finishRead] of [
    ['rejected fetch', read => read.reject(new Error('source unavailable'))],
    ['HTTP error', read => read.resolve(new Response(null, { status: 404 }))],
]) {
    test(`EmbeddedRuntimeManager counts an unrecoverable live page until disposal: ${failure}`, async (t) => {
        const { dom, manager, read, a, b } = prepareCapacityHandoff(t);
        t.mock.method(console, 'warn', () => {});
        finishRead(read);
        await settleReads();
        dom.flushRaf();
        assert.equal(a.host.querySelector('iframe'), a.iframe);
        assert.equal(b.host.querySelector('iframe'), null);
        assert.equal(manager.getPerfSnapshot().activeIframes, 1);

        manager.unregister('a');
        dom.flushRaf();
        assert.equal(a.host.querySelector('iframe'), null);
        assert.equal(b.host.querySelector('iframe'), b.iframe);
        assert.equal(manager.getPerfSnapshot().activeIframes, 1);
    });
}

test('EmbeddedRuntimeManager keeps a healthy resident when a preferred missing page is pending or failed', async (t) => {
    const { dom, manager, create } = setupManagedIframes(t);
    const read = deferSourceRead(t);
    t.mock.method(console, 'warn', () => {});
    const healthy = create('b', { srcdoc: '<p>healthy</p>' }, true);
    manager.reconcile();
    const missing = create('a', { src: 'blob:pending' }, false);
    missing.iframe.remove();
    manager.invalidate('a');
    manager.setVisible('a', true);
    manager.touch('a');
    manager.reconcile();
    assert.equal(healthy.host.querySelector('iframe'), healthy.iframe);
    assert.equal(missing.host.querySelector('iframe'), null);
    assert.equal(manager.getPerfSnapshot().activeIframes, 1);

    read.reject(new Error('source unavailable'));
    await settleReads();
    dom.flushRaf();
    manager.reconcile();
    assert.equal(healthy.host.querySelector('iframe'), healthy.iframe);
    assert.equal(missing.host.querySelector('iframe'), null);
    assert.equal(manager.getPerfSnapshot().activeIframes, 1);
    const placeholder = missing.host.querySelector('.tt-runtime-placeholder');
    assert.match(placeholder.textContent, /Cannot restore this page locally/);
    assert.equal(placeholder.tabIndex, -1);
});
