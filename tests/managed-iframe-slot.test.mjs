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
        iframe.src = 'blob:runtime';
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

test('managed iframe slot: cold rebuild replaces a transient blob iframe instead of reviving it', async () => {
    const dom = installFakeDom();
    const id = 'slot:test:blob-cold-rebuild';
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
        iframe.src = 'blob:transient-runtime';
        host.append(iframe);
        document.body.append(host);

        let replacement = null;
        const slot = createManagedIframeSlot({
            id,
            kind: 'k',
            host,
            maxSoftParkedIframes: 2,
            softParkTtlMs: 1000,
            requestColdRebuild: () => {
                replacement = document.createElement('iframe');
                replacement.src = 'blob:fresh-runtime';
                host.append(replacement);
            },
        });

        slot.hydrate();
        slot.dehydrate('budget');

        assert.equal(host.querySelector('iframe'), null);
        assert.equal(iframe.isConnected, false);

        slot.hydrate();
        assert.ok(replacement);
        assert.equal(host.querySelector('iframe'), replacement);
    } finally {
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
