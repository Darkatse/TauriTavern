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

async function importStable(modulePath) {
    return import(pathToFileURL(modulePath).href);
}

test('managed iframe slot: budget park uses a placeholder and restores the parked iframe on hydrate', async () => {
    const dom = installFakeDom();
    const id = 'slot:test:budget';
    try {
        const { createManagedIframeSlot } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js'),
        );
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );
        lot.dropParkedManagedIframe(id);

        const host = document.createElement('div');
        document.body.append(host);

        const iframe = document.createElement('iframe');
        iframe.offsetHeight = 123;
        iframe.srcdoc = '<p>runtime</p>';
        host.append(iframe);

        const slot = createManagedIframeSlot({
            id,
            kind: 'k',
            host,
            maxSoftParkedIframes: 2,
            softParkTtlMs: 1000,
        });

        slot.hydrate();
        slot.dehydrate('budget');

        const placeholder = host.querySelector('.tt-runtime-placeholder');
        assert.ok(placeholder);
        assert.equal(host.querySelector('iframe'), null);
        assert.equal(placeholder.style.minHeight, '123px');
        assert.equal(placeholder.dataset.ttRuntimeParkReason, 'budget');

        slot.hydrate();
        assert.equal(host.querySelector('.tt-runtime-placeholder'), null);
        assert.equal(host.querySelector('iframe'), iframe);
    } finally {
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );
        lot.dropParkedManagedIframe(id);
        dom.cleanup();
    }
});

test('managed iframe slot: srcdoc cold recovery keeps the renderer element', async () => {
    const dom = installFakeDom();
    let slot;
    try {
        const { createManagedIframeSlot } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js'),
        );
        const host = document.createElement('div');
        const iframe = document.createElement('iframe');
        iframe.srcdoc = '<p>srcdoc page</p>';
        host.append(iframe);
        document.body.append(host);
        slot = createManagedIframeSlot({
            id: 'slot:srcdoc:cold', kind: 'k', host,
            maxSoftParkedIframes: 0, softParkTtlMs: 0,
        });
        await slot.hydrate();
        slot.dehydrate('visibility');
        assert.equal(iframe.isConnected, false);
        await slot.hydrate();
        assert.equal(host.querySelector('iframe'), iframe);
        assert.equal(iframe.srcdoc, '<p>srcdoc page</p>');
        slot.dehydrate('visibility');
        const otherHost = document.createElement('div');
        document.body.append(otherHost);
        otherHost.append(iframe);
        await slot.hydrate();
        assert.equal(otherHost.querySelector('iframe'), iframe, 'do not steal a page moved to another owner');
        assert.equal(host.querySelector('.tt-runtime-placeholder').dataset.ttRuntimeParkReason, 'source-unavailable');
    } finally {
        slot?.dispose();
        dom.cleanup();
    }
});

test('managed iframe slot: restores revoked blob source locally and releases its owned URL', async () => {
    const nativeQueueMicrotask = globalThis.queueMicrotask;
    const dom = installFakeDom();
    globalThis.queueMicrotask = nativeQueueMicrotask;
    const id = 'slot:test:blob-cold-rebuild';
    const sourceUrl = URL.createObjectURL(new Blob(['<p>original source</p>'], { type: 'text/html' }));
    let slot;
    try {
        const { createManagedIframeSlot } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js'),
        );
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );
        lot.dropParkedManagedIframe(id);

        const host = document.createElement('div');
        const iframe = document.createElement('iframe');
        iframe.src = sourceUrl;
        host.append(iframe);
        document.body.append(host);

        slot = createManagedIframeSlot({
            id,
            kind: 'k',
            host,
            maxSoftParkedIframes: 2,
            softParkTtlMs: 1000,
        });

        await slot.hydrate();
        slot.dehydrate('budget');
        URL.revokeObjectURL(sourceUrl);

        assert.equal(host.querySelector('iframe'), null);
        assert.equal(iframe.isConnected, false);

        await slot.hydrate();
        assert.equal(host.querySelector('iframe'), iframe);
        assert.notEqual(iframe.src, sourceUrl);
        assert.equal(await (await fetch(iframe.src)).text(), '<p>original source</p>');
        const ownedUrl = iframe.src;
        slot.dehydrate('visibility');
        await slot.hydrate();
        assert.notEqual(iframe.src, ownedUrl);
        await assert.rejects(fetch(ownedUrl), 'superseded object URLs must be released');
        const restoredUrl = iframe.src;
        assert.equal(await (await fetch(restoredUrl)).text(), '<p>original source</p>');
        slot.dispose();
        await assert.rejects(fetch(restoredUrl));
    } finally {
        slot?.dispose();
        URL.revokeObjectURL(sourceUrl);
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );
        lot.dropParkedManagedIframe(id);
        dom.cleanup();
    }
});

test('managed iframe slot: hydrate keeps an upstream replacement over a parked iframe', async () => {
    const dom = installFakeDom();
    const id = 'slot:test:upstream-replacement';
    try {
        const { createManagedIframeSlot } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js'),
        );
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );
        lot.dropParkedManagedIframe(id);

        const host = document.createElement('div');
        const parkedIframe = document.createElement('iframe');
        parkedIframe.src = 'https://old.example/';
        host.append(parkedIframe);
        document.body.append(host);

        const slot = createManagedIframeSlot({
            id,
            kind: 'k',
            host,
            maxSoftParkedIframes: 2,
            softParkTtlMs: 1000,
        });

        slot.hydrate();
        slot.dehydrate('budget');

        const replacement = document.createElement('iframe');
        replacement.src = 'https://new.example/';
        host.append(replacement);

        slot.hydrate();

        assert.equal(host.querySelector('iframe'), replacement);
        assert.equal(parkedIframe.isConnected, false);
        assert.equal(host.querySelector('.tt-runtime-placeholder'), null);
    } finally {
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );
        lot.dropParkedManagedIframe(id);
        dom.cleanup();
    }
});

for (const interruption of ['park', 'dispose', 'replacement']) {
    test(`managed iframe slot: pending blob recovery cannot overwrite ${interruption}`, async (t) => {
        const dom = installFakeDom();
        let finishCapture;
        const capture = new Promise(resolve => { finishCapture = resolve; });
        t.mock.method(globalThis, 'fetch', async () => {
            await capture;
            return { ok: true, blob: async () => new Blob(['<p>old</p>']) };
        });
        let slot;
        try {
            const { createManagedIframeSlot } = await importFresh(
                path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js'),
            );
            const host = document.createElement('div');
            const iframe = document.createElement('iframe');
            iframe.src = 'blob:delayed-source';
            host.append(iframe);
            document.body.append(host);
            slot = createManagedIframeSlot({
                id: `slot:pending:${interruption}`, kind: 'k', host,
                maxSoftParkedIframes: 2, softParkTtlMs: 1000,
            });
            const capturing = slot.hydrate();
            // An external renderer can remove its frame while the source read
            // is still pending. The slot must cancel stale recovery work.
            iframe.remove();
            const restoring = slot.hydrate();
            let replacement = null;
            if (interruption === 'park') {
                slot.dehydrate('visibility');
            } else if (interruption === 'dispose') {
                slot.dispose();
            } else {
                replacement = document.createElement('iframe');
                replacement.srcdoc = '<p>new content</p>';
                host.append(replacement);
            }
            finishCapture();
            await capturing;
            await restoring;
            assert.equal(host.querySelector('iframe'), replacement);
            assert.equal(iframe.isConnected, false);
            if (interruption === 'park') {
                await slot.hydrate();
                assert.equal(host.querySelector('iframe'), iframe);
            } else if (interruption === 'replacement') {
                slot.dehydrate('visibility');
                await slot.hydrate();
                assert.equal(host.querySelector('iframe'), replacement);
            }
        } finally {
            slot?.dispose();
            dom.cleanup();
        }
    });
}

for (const failure of ['reject', 'http']) {
    test(`managed iframe slot: ${failure} source failure retains the live page and reports external removal locally`, async (t) => {
        const dom = installFakeDom();
        t.mock.method(globalThis, 'fetch', async () => {
            if (failure === 'reject') throw new Error('source revoked');
            return { ok: false, status: 404 };
        });
        let slot;
        try {
            const { createManagedIframeSlot } = await importFresh(
                path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js'),
            );
            const host = document.createElement('div');
            const iframe = document.createElement('iframe');
            iframe.src = 'blob:unavailable';
            host.append(iframe);
            document.body.append(host);
            slot = createManagedIframeSlot({
                id: `slot:failure:${failure}`, kind: 'k', host,
                maxSoftParkedIframes: 2, softParkTtlMs: 1000,
            });
            const capturing = slot.hydrate();
            slot.dehydrate('visibility');
            assert.equal(host.querySelector('iframe'), iframe, 'pending capture must not discard the page');
            await capturing;
            slot.dehydrate('budget');
            assert.equal(host.querySelector('iframe'), iframe, 'failed capture must not discard the page');
            iframe.remove();
            await slot.hydrate();
            assert.equal(host.querySelector('.tt-runtime-ghost'), null);
            assert.equal(host.querySelector('.tt-runtime-placeholder').dataset.ttRuntimeParkReason, 'source-unavailable');
            assert.match(host.querySelector('.tt-runtime-placeholder-hint').textContent, /Reopen the chat/);
        } finally {
            slot?.dispose();
            dom.cleanup();
        }
    });
}

test('managed iframe slot: pending capture parks only after source bytes are safe', async (t) => {
    const dom = installFakeDom();
    let finishCapture;
    const capture = new Promise(resolve => { finishCapture = resolve; });
    t.mock.method(globalThis, 'fetch', async () => {
        await capture;
        return { ok: true, blob: async () => new Blob(['<p>safe source</p>']) };
    });
    let slot;
    try {
        const { createManagedIframeSlot } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js'),
        );
        const host = document.createElement('div');
        const iframe = document.createElement('iframe');
        iframe.src = 'blob:pending';
        host.append(iframe);
        document.body.append(host);
        slot = createManagedIframeSlot({
            id: 'slot:pending:park', kind: 'k', host,
            maxSoftParkedIframes: 2, softParkTtlMs: 1000,
        });
        const capturing = slot.hydrate();
        slot.dehydrate('visibility');
        assert.equal(host.querySelector('iframe'), iframe);
        finishCapture();
        await capturing;
        assert.equal(host.querySelector('iframe'), null);
        await slot.hydrate();
        assert.equal(host.querySelector('iframe'), iframe);
    } finally {
        slot?.dispose();
        dom.cleanup();
    }
});

test('managed iframe slot: an upstream element can inherit the owned blob URL', async () => {
    const nativeQueueMicrotask = globalThis.queueMicrotask;
    const dom = installFakeDom();
    globalThis.queueMicrotask = nativeQueueMicrotask;
    const sourceUrl = URL.createObjectURL(new Blob(['<p>shared source</p>'], { type: 'text/html' }));
    let slot;
    try {
        const { createManagedIframeSlot } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js'),
        );
        const host = document.createElement('div');
        const iframe = document.createElement('iframe');
        iframe.src = sourceUrl;
        host.append(iframe);
        document.body.append(host);
        slot = createManagedIframeSlot({
            id: 'slot:owned:replacement', kind: 'k', host,
            maxSoftParkedIframes: 2, softParkTtlMs: 1000,
        });
        await slot.hydrate();
        slot.dehydrate('visibility');
        URL.revokeObjectURL(sourceUrl);
        await slot.hydrate();
        const ownedUrl = iframe.src;
        const replacement = document.createElement('iframe');
        replacement.src = ownedUrl;
        iframe.replaceWith(replacement);
        await slot.hydrate();
        assert.equal(await (await fetch(ownedUrl)).text(), '<p>shared source</p>');
        slot.dehydrate('visibility');
        await slot.hydrate();
        assert.equal(host.querySelector('iframe'), replacement);
        assert.equal(await (await fetch(replacement.src)).text(), '<p>shared source</p>');
        await assert.rejects(fetch(ownedUrl));
    } finally {
        slot?.dispose();
        URL.revokeObjectURL(sourceUrl);
        dom.cleanup();
    }
});

test('managed iframe slot: dispose destroys active and parked iframe ownership', async () => {
    const dom = installFakeDom();
    const activeId = 'slot:test:dispose-active';
    const parkedId = 'slot:test:dispose-parked';
    try {
        const { createManagedIframeSlot } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js'),
        );
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );

        const createSlot = (id) => {
            const host = document.createElement('div');
            const iframe = document.createElement('iframe');
            host.append(iframe);
            document.body.append(host);
            return {
                host,
                iframe,
                slot: createManagedIframeSlot({
                    id,
                    kind: 'k',
                    host,
                    maxSoftParkedIframes: 2,
                    softParkTtlMs: 1000,
                }),
            };
        };

        const active = createSlot(activeId);
        active.slot.dispose();
        assert.equal(active.host.querySelector('iframe'), null);
        assert.equal(active.iframe.isConnected, false);
        assert.equal(lot.takeParkedManagedIframe(activeId), null);

        const parked = createSlot(parkedId);
        parked.slot.dehydrate('visibility');
        assert.equal(parked.iframe.isConnected, true);
        parked.slot.dispose();
        parked.slot.dispose();
        assert.equal(parked.host.querySelector('.tt-runtime-ghost'), null);
        assert.equal(parked.iframe.isConnected, false);
        assert.equal(lot.takeParkedManagedIframe(parkedId), null);
    } finally {
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );
        lot.dropParkedManagedIframe(activeId);
        lot.dropParkedManagedIframe(parkedId);
        dom.cleanup();
    }
});
