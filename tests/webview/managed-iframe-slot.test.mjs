import { afterEach, expect, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const { createManagedIframeSlot } = await importFrontend('tauri/main/adapters/embedded-runtime/managed-iframe-slot.js');
const { takeParkedManagedIframe } = await importFrontend('tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js');

const slots = [];
afterEach(() => {
    for (const slot of slots.splice(0)) slot.dispose();
});

function create(attributes = { srcdoc: '<p>runtime</p>' }, maxSoftParkedIframes = 0) {
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
}

const read = async url => (await fetch(url)).text();

test('managed iframe slot: budget park preserves height and restores the parked element', () => {
    const { host, iframe, slot } = create({ srcdoc: '<p>runtime</p>' }, 2);
    iframe.style.cssText = 'height: 123px; border: 0';
    slot.hydrate();
    slot.dehydrate('budget');
    const placeholder = host.querySelector('.tt-runtime-placeholder');
    expect(host.querySelector('iframe')).toBeNull();
    expect(placeholder.style.minHeight).toBe('123px');
    slot.hydrate();
    expect(host.querySelector('.tt-runtime-placeholder')).toBeNull();
    expect(host.querySelector('iframe')).toBe(iframe);
});

for (const maxSoftParkedIframes of [0, 2]) {
    test(`managed iframe slot: srcdoc recovery respects a subsequent owner (soft capacity ${maxSoftParkedIframes})`, () => {
        const { host, iframe, slot } = create({ srcdoc: '<p>runtime</p>' }, maxSoftParkedIframes);
        slot.dehydrate('visibility');
        expect(host.querySelector('iframe')).toBeNull();
        slot.hydrate();
        expect(host.querySelector('iframe')).toBe(iframe);
        expect(iframe.srcdoc).toBe('<p>runtime</p>');
        slot.dehydrate('visibility');
        const otherHost = document.createElement('div');
        document.body.append(otherHost);
        otherHost.append(iframe);
        slot.hydrate();
        expect(host.querySelector('.tt-runtime-placeholder').dataset.ttRuntimeParkReason).toBe('source-unavailable');
        slot.dispose();
        expect(otherHost.querySelector('iframe')).toBe(iframe);
    });
}

test('managed iframe slot: restores revoked blob source locally and releases its owned URLs', async () => {
    const sourceUrl = URL.createObjectURL(new Blob(['<p>original source</p>'], { type: 'text/html' }));
    const { host, iframe, slot, sourceReady } = create({ src: sourceUrl }, 2);
    await sourceReady;
    slot.dehydrate('budget');
    URL.revokeObjectURL(sourceUrl);
    expect(host.querySelector('iframe')).toBeNull();
    expect(iframe.isConnected).toBe(false);
    slot.hydrate();
    expect(host.querySelector('iframe')).toBe(iframe);
    expect(iframe.src).not.toBe(sourceUrl);
    expect(await read(iframe.src)).toBe('<p>original source</p>');
    const ownedUrl = iframe.src;
    slot.dehydrate('visibility');
    slot.hydrate();
    const restoredUrl = iframe.src;
    expect(await read(restoredUrl)).toBe('<p>original source</p>');
    if (restoredUrl !== ownedUrl) await expect(fetch(ownedUrl)).rejects.toThrow();
    slot.dispose();
    for (const url of new Set([ownedUrl, restoredUrl])) await expect(fetch(url)).rejects.toThrow();
});

test('managed iframe slot: a captured replacement supersedes the parked element even if removed before recovery', () => {
    const { host, iframe, slot } = create({ srcdoc: '<p>old</p>' }, 2);
    slot.dehydrate('budget');
    const replacement = document.createElement('iframe');
    replacement.srcdoc = '<p>new</p>';
    host.append(replacement);
    slot.refresh();
    replacement.remove();
    slot.hydrate();
    expect(host.querySelector('iframe'), 'restore the latest renderer element').toBe(replacement);
    expect(iframe.isConnected).toBe(false);
    expect(host.querySelector('.tt-runtime-placeholder')).toBeNull();
});

test('managed iframe slot: an upstream element can inherit the owned blob URL', async () => {
    const sourceUrl = URL.createObjectURL(new Blob(['<p>shared source</p>'], { type: 'text/html' }));
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
    expect(await read(ownedUrl)).toBe('<p>shared source</p>');
    slot.dehydrate('visibility');
    slot.hydrate();
    expect(host.querySelector('iframe')).toBe(replacement);
    expect(await read(replacement.src)).toBe('<p>shared source</p>');
    if (replacement.src !== ownedUrl) await expect(fetch(ownedUrl)).rejects.toThrow();
});

test('managed iframe slot: dispose destroys active and parked iframe ownership', () => {
    const active = create();
    active.slot.dispose();
    expect(active.host.querySelector('iframe')).toBeNull();
    expect(active.iframe.isConnected).toBe(false);
    const parked = create({ srcdoc: '<p>runtime</p>' }, 2);
    parked.slot.dehydrate('visibility');
    expect(parked.iframe.isConnected).toBe(true);
    parked.slot.dispose();
    parked.slot.dispose();
    expect(parked.host.querySelector('.tt-runtime-ghost')).toBeNull();
    expect(parked.iframe.isConnected).toBe(false);
    expect(takeParkedManagedIframe(parked.slot.id)).toBeNull();
});
