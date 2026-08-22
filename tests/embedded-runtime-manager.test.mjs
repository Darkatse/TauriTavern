import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { installFakeDom } from './helpers/fake-dom.mjs';

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
