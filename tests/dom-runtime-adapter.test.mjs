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

test('dom embedded-runtime adapter: scanForHosts registers nearest + descendant hosts', async () => {
    const dom = installFakeDom();
    try {
        const { scanForHosts } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/dom-runtime-adapter.js'),
        );

        const hostA = document.createElement('div');
        hostA.classList.add('host');
        document.body.append(hostA);

        const root = document.createElement('div');
        hostA.append(root);

        const hostB = document.createElement('div');
        hostB.classList.add('host');
        root.append(hostB);

        const seen = [];
        scanForHosts({}, root, [{
            hostSelector: '.host',
            registerHost(_manager, host) {
                seen.push(host);
            },
        }]);

        assert.deepEqual(seen, [hostA, hostB]);
    } finally {
        dom.cleanup();
    }
});

test('dom embedded-runtime adapter: unregisterSlotsInSubtree unregisters and clears ids, except moving', async () => {
    const dom = installFakeDom();
    try {
        const { unregisterSlotsInSubtree } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/dom-runtime-adapter.js'),
        );

        const root = document.createElement('div');
        document.body.append(root);

        const a = document.createElement('div');
        a.dataset.ttRuntimeSlotId = 'a';
        const b = document.createElement('div');
        b.dataset.ttRuntimeSlotId = 'b';
        b.dataset.ttRuntimeMoving = '1';
        root.append(a, b);

        const calls = [];
        const manager = {
            unregister(id) {
                calls.push(id);
            },
        };

        unregisterSlotsInSubtree(manager, root);

        assert.deepEqual(calls, ['a']);
        assert.equal(a.dataset.ttRuntimeSlotId, undefined);
        assert.equal(b.dataset.ttRuntimeSlotId, 'b');
    } finally {
        dom.cleanup();
    }
});

test('dom embedded-runtime adapter: installDomEmbeddedRuntimeAdapter touches on placeholder click and tracks mutations', async () => {
    const dom = installFakeDom();
    try {
        const { installDomEmbeddedRuntimeAdapter } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/dom-runtime-adapter.js'),
        );

        const root = document.createElement('div');
        document.body.append(root);

        const host = document.createElement('div');
        host.classList.add('host');
        host.dataset.ttRuntimeSlotId = 'slot1';

        const placeholder = document.createElement('div');
        placeholder.classList.add('tt-runtime-placeholder');
        host.append(placeholder);
        root.append(host);

        const touched = [];
        const unregistered = [];
        const registeredHosts = [];

        const manager = {
            touch(id) {
                touched.push(id);
            },
            unregister(id) {
                unregistered.push(id);
            },
        };

        const adapter = {
            hostSelector: '.host',
            registerHost(_manager, el) {
                registeredHosts.push(el);
            },
        };

        const handle = installDomEmbeddedRuntimeAdapter({ manager, root, adapters: [adapter] });

        assert.deepEqual(registeredHosts, [host]);

        root.dispatchEvent({ type: 'click', target: placeholder });
        assert.deepEqual(touched, ['slot1']);

        const observer = dom.createdMutationObservers.at(-1);
        assert.ok(observer);

        const removed = document.createElement('div');
        removed.dataset.ttRuntimeSlotId = 'gone';
        observer._trigger([{ target: root, removedNodes: [removed], addedNodes: [] }]);
        assert.deepEqual(unregistered, ['gone']);
        assert.equal(removed.dataset.ttRuntimeSlotId, undefined);

        const added = document.createElement('div');
        added.classList.add('host');
        observer._trigger([{ target: root, removedNodes: [], addedNodes: [added] }]);
        assert.equal(registeredHosts.includes(added), true);

        handle.dispose();
    } finally {
        dom.cleanup();
    }
});

