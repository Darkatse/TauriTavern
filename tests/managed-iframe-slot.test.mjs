import test from 'node:test';
import assert from 'node:assert/strict';
import { installFakeDom } from './helpers/fake-dom.mjs';
import { createManagedIframeSlot } from '../src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js';
import { takeParkedManagedIframe } from '../src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js';

function setup(t) {
    const nativeQueueMicrotask = globalThis.queueMicrotask;
    const dom = installFakeDom();
    globalThis.queueMicrotask = nativeQueueMicrotask;
    const slots = [];
    t.after(() => {
        for (const slot of slots) slot.dispose();
        dom.cleanup();
    });
    return (attributes = { srcdoc: '<p>runtime</p>' }, maxSoftParkedIframes = 0) => {
        const host = document.createElement('div');
        const iframe = document.createElement('iframe');
        Object.assign(iframe, attributes);
        host.append(iframe);
        document.body.append(host);
        const ready = Promise.withResolvers();
        const slot = createManagedIframeSlot({
            id: `slot:${slots.length}`, kind: 'k', host,
            maxSoftParkedIframes, softParkTtlMs: 1000,
            onSourceSettled: ready.resolve,
        });
        slots.push(slot);
        return { host, iframe, slot, sourceReady: ready.promise };
    };
}

test('managed iframe slot: budget park preserves height and restores the parked element', (t) => {
    const create = setup(t);
    const { host, iframe, slot } = create({ srcdoc: '<p>runtime</p>' }, 2);
    iframe.offsetHeight = 123;
    slot.hydrate();
    slot.dehydrate('budget');
    const placeholder = host.querySelector('.tt-runtime-placeholder');
    assert.equal(host.querySelector('iframe'), null);
    assert.equal(placeholder.style.minHeight, '123px');
    slot.hydrate();
    assert.equal(host.querySelector('.tt-runtime-placeholder'), null);
    assert.equal(host.querySelector('iframe'), iframe);
});

for (const maxSoftParkedIframes of [0, 2]) {
    test(`managed iframe slot: srcdoc recovery respects a subsequent owner (soft capacity ${maxSoftParkedIframes})`, (t) => {
        const create = setup(t);
        const { host, iframe, slot } = create({ srcdoc: '<p>runtime</p>' }, maxSoftParkedIframes);
        slot.dehydrate('visibility');
        assert.equal(host.querySelector('iframe'), null);
        slot.hydrate();
        assert.equal(host.querySelector('iframe'), iframe);
        assert.equal(iframe.srcdoc, '<p>runtime</p>');
        slot.dehydrate('visibility');
        const otherHost = document.createElement('div');
        document.body.append(otherHost);
        otherHost.append(iframe);
        slot.hydrate();
        assert.equal(host.querySelector('.tt-runtime-placeholder').dataset.ttRuntimeParkReason, 'source-unavailable');
        slot.dispose();
        assert.equal(otherHost.querySelector('iframe'), iframe);
    });
}

test('managed iframe slot: restores revoked blob source locally and releases its owned URLs', async (t) => {
    const create = setup(t);
    const sourceUrl = URL.createObjectURL(new Blob(['<p>original source</p>'], { type: 'text/html' }));
    t.after(() => URL.revokeObjectURL(sourceUrl));
    const { host, iframe, slot, sourceReady } = create({ src: sourceUrl }, 2);
    await sourceReady;
    slot.dehydrate('budget');
    URL.revokeObjectURL(sourceUrl);
    assert.equal(host.querySelector('iframe'), null);
    assert.equal(iframe.isConnected, false);
    slot.hydrate();
    assert.equal(host.querySelector('iframe'), iframe);
    assert.notEqual(iframe.src, sourceUrl);
    assert.equal(await (await fetch(iframe.src)).text(), '<p>original source</p>');
    const ownedUrl = iframe.src;
    slot.dehydrate('visibility');
    slot.hydrate();
    const restoredUrl = iframe.src;
    assert.equal(await (await fetch(restoredUrl)).text(), '<p>original source</p>');
    if (restoredUrl !== ownedUrl) await assert.rejects(fetch(ownedUrl));
    slot.dispose();
    for (const url of new Set([ownedUrl, restoredUrl])) await assert.rejects(fetch(url));
});

test('managed iframe slot: a captured replacement supersedes the parked element even if removed before recovery', (t) => {
    const create = setup(t);
    const { host, iframe, slot } = create({ srcdoc: '<p>old</p>' }, 2);
    slot.dehydrate('budget');
    const replacement = document.createElement('iframe');
    replacement.srcdoc = '<p>new</p>';
    host.append(replacement);
    slot.refresh();
    replacement.remove();
    slot.hydrate();
    assert.ok(host.querySelector('iframe') === replacement, 'restore the latest renderer element');
    assert.equal(iframe.isConnected, false);
    assert.equal(host.querySelector('.tt-runtime-placeholder'), null);
});

test('managed iframe slot: an upstream element can inherit the owned blob URL', async (t) => {
    const create = setup(t);
    const sourceUrl = URL.createObjectURL(new Blob(['<p>shared source</p>'], { type: 'text/html' }));
    t.after(() => URL.revokeObjectURL(sourceUrl));
    const { host, iframe, slot, sourceReady } = create({ src: sourceUrl });
    await sourceReady;
    slot.dehydrate('visibility');
    URL.revokeObjectURL(sourceUrl);
    slot.hydrate();
    const ownedUrl = iframe.src;
    const replacement = document.createElement('iframe');
    replacement.src = ownedUrl;
    iframe.replaceWith(replacement);
    slot.hydrate();
    assert.equal(await (await fetch(ownedUrl)).text(), '<p>shared source</p>');
    slot.dehydrate('visibility');
    slot.hydrate();
    assert.equal(host.querySelector('iframe'), replacement);
    assert.equal(await (await fetch(replacement.src)).text(), '<p>shared source</p>');
    if (replacement.src !== ownedUrl) await assert.rejects(fetch(ownedUrl));
});

test('managed iframe slot: dispose destroys active and parked iframe ownership', (t) => {
    const create = setup(t);
    const active = create();
    active.slot.dispose();
    assert.equal(active.host.querySelector('iframe'), null);
    assert.equal(active.iframe.isConnected, false);
    const parked = create({ srcdoc: '<p>runtime</p>' }, 2);
    parked.slot.dehydrate('visibility');
    assert.equal(parked.iframe.isConnected, true);
    parked.slot.dispose();
    parked.slot.dispose();
    assert.equal(parked.host.querySelector('.tt-runtime-ghost'), null);
    assert.equal(parked.iframe.isConnected, false);
    assert.equal(takeParkedManagedIframe(parked.slot.id), null);
});
