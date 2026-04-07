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

test('parseRootMarginPx supports 1-4 px values and rejects invalid input', async () => {
    const dom = installFakeDom();
    try {
        const { parseRootMarginPx } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-normalize.js'),
        );

        assert.deepEqual(parseRootMarginPx(''), { top: 0, right: 0, bottom: 0, left: 0 });
        assert.deepEqual(parseRootMarginPx('10px'), { top: 10, right: 10, bottom: 10, left: 10 });
        assert.deepEqual(parseRootMarginPx('10px 20px'), { top: 10, right: 20, bottom: 10, left: 20 });
        assert.deepEqual(parseRootMarginPx('1px 2px 3px'), { top: 1, right: 2, bottom: 3, left: 2 });
        assert.deepEqual(parseRootMarginPx('1px 2px 3px 4px'), { top: 1, right: 2, bottom: 3, left: 4 });

        assert.throws(() => parseRootMarginPx('10px 20px 30px 40px 50px'), /Invalid rootMargin/);
        assert.throws(() => parseRootMarginPx('10%'), /only supports px/);
    } finally {
        dom.cleanup();
    }
});

test('compareEmbeddedRuntimeSlotRank orders by viewport > visible > priority > touch > visibleAt > id', async () => {
    const dom = installFakeDom();
    try {
        const { compareEmbeddedRuntimeSlotRank } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-normalize.js'),
        );

        const base = { inViewport: false, visible: false, priority: 0, lastVisibleAt: 0, lastTouchedAt: 0 };
        const items = [
            { ...base, id: 'b' },
            { ...base, id: 'a' },
            { ...base, id: 'v', visible: true },
            { ...base, id: 'p', visible: true, priority: 10 },
            { ...base, id: 't', visible: true, priority: 10, lastTouchedAt: 5 },
            { ...base, id: 'x', visible: true, priority: 10, lastTouchedAt: 5, inViewport: true },
        ];

        const sorted = items.slice().sort(compareEmbeddedRuntimeSlotRank).map((x) => x.id);
        assert.deepEqual(sorted, ['x', 't', 'p', 'v', 'a', 'b']);
    } finally {
        dom.cleanup();
    }
});

test('normalizeEmbeddedRuntimeSlot fails fast on invalid input and clamps numeric fields', async () => {
    const dom = installFakeDom();
    try {
        const { normalizeEmbeddedRuntimeSlot } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-normalize.js'),
        );

        assert.throws(() => normalizeEmbeddedRuntimeSlot(null), /slot is required/);
        assert.throws(
            () => normalizeEmbeddedRuntimeSlot({ id: 'a', kind: 'k', element: {}, hydrate() {}, dehydrate() {} }),
            /slot\.element must be an HTMLElement/,
        );

        const element = document.createElement('div');
        const normalized = normalizeEmbeddedRuntimeSlot({
            id: 'a',
            kind: 'k',
            element,
            hydrate() {},
            dehydrate() {},
            weight: -1,
            priority: 999999,
            iframeCount: 999999,
        });

        assert.equal(normalized.id, 'a');
        assert.equal(normalized.kind, 'k');
        assert.equal(normalized.weight, 0);
        assert.equal(normalized.priority, 1000);
        assert.equal(normalized.iframeCount, 1000);
    } finally {
        dom.cleanup();
    }
});

test('normalizeEmbeddedRuntimeProfile clamps budgets and converts parkWhenHiddenKinds to a Set', async () => {
    const dom = installFakeDom();
    try {
        const { normalizeEmbeddedRuntimeProfile } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-normalize.js'),
        );

        const normalized = normalizeEmbeddedRuntimeProfile({
            name: 'p',
            maxActiveWeight: -1,
            maxActiveIframes: 'x',
            maxActiveSlots: 2,
            maxSoftParkedIframes: 3,
            softParkTtlMs: 9999999999,
            parkWhenHiddenKinds: ['a', '  ', 'b'],
            rootMargin: 123,
            threshold: 2,
        });

        assert.equal(normalized.name, 'p');
        assert.equal(normalized.maxActiveWeight, 0);
        assert.equal(normalized.maxActiveIframes, 0);
        assert.equal(normalized.maxActiveSlots, 2);
        assert.equal(normalized.maxSoftParkedIframes, 3);
        assert.equal(normalized.softParkTtlMs, 60 * 60 * 1000);
        assert.ok(normalized.parkWhenHiddenKinds instanceof Set);
        assert.ok(normalized.parkWhenHiddenKinds.has('a'));
        assert.ok(normalized.parkWhenHiddenKinds.has('b'));
        assert.equal(normalized.rootMargin, '200px 0px');
        assert.equal(normalized.threshold, 1);
    } finally {
        dom.cleanup();
    }
});

