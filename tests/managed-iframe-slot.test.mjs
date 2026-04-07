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

        dom.flushMicrotasks();
        assert.equal(iframe.dataset.ttRuntimeManaged, undefined);

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

test('managed iframe slot: visibility park switches from budget placeholder to ghost placeholder', async () => {
    const dom = installFakeDom();
    const id = 'slot:test:visibility';
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
        iframe.offsetHeight = 111;
        host.append(iframe);

        const slot = createManagedIframeSlot({
            id,
            kind: 'k',
            host,
            maxSoftParkedIframes: 1,
            softParkTtlMs: 1000,
        });

        slot.hydrate();
        slot.dehydrate('budget');
        dom.flushMicrotasks();

        assert.ok(host.querySelector('.tt-runtime-placeholder'));
        slot.dehydrate('visibility');

        assert.equal(host.querySelector('.tt-runtime-placeholder'), null);
        const ghost = host.querySelector('.tt-runtime-ghost');
        assert.ok(ghost);
        assert.equal(ghost.style.minHeight, '111px');

        slot.hydrate();
        assert.equal(host.querySelector('.tt-runtime-ghost'), null);
        assert.equal(host.querySelector('iframe'), iframe);
    } finally {
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );
        lot.dropParkedManagedIframe(id);
        dom.cleanup();
    }
});

test('managed iframe slot: cold start calls requestColdRebuild instead of cloning a stale template', async () => {
    const dom = installFakeDom();
    const id = 'slot:test:cold-rebuild';
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
        iframe.src = 'blob:stale';
        host.append(iframe);

        const calls = [];
        const slot = createManagedIframeSlot({
            id,
            kind: 'k',
            host,
            maxSoftParkedIframes: 0,
            softParkTtlMs: 0,
            requestColdRebuild: () => calls.push('cold'),
        });

        slot.hydrate(); // materializes template
        iframe.remove();

        slot.hydrate();
        assert.deepEqual(calls, ['cold']);
        assert.equal(host.querySelector('iframe'), null);
    } finally {
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );
        lot.dropParkedManagedIframe(id);
        dom.cleanup();
    }
});

test('managed iframe slot: cold start clones the template when no requestColdRebuild is provided', async () => {
    const dom = installFakeDom();
    const id = 'slot:test:cold-clone';
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
        iframe.src = 'blob:template';
        host.append(iframe);

        const slot = createManagedIframeSlot({
            id,
            kind: 'k',
            host,
            maxSoftParkedIframes: 0,
            softParkTtlMs: 0,
        });

        slot.hydrate(); // materializes template
        iframe.remove();

        slot.hydrate();

        const clone = host.querySelector('iframe');
        assert.ok(clone);
        assert.notEqual(clone, iframe);
        assert.equal(clone.dataset.ttRuntimeClone, '1');
    } finally {
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );
        lot.dropParkedManagedIframe(id);
        dom.cleanup();
    }
});

