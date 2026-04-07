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

test('managed iframe parking-lot keeps browsing contexts alive in a hidden container', async () => {
    const dom = installFakeDom();
    try {
        const lotMod = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );

        const iframe = document.createElement('iframe');
        lotMod.parkManagedIframe({ id: 'a', iframe, maxIframes: 10, ttlMs: 0 });

        const lot = document.getElementById('tt-embedded-runtime-iframe-parking-lot');
        assert.ok(lot);
        assert.equal(lot.isConnected, true);
        assert.equal(lotMod.getParkedManagedIframeCount(), 1);
        assert.equal(iframe.isConnected, true);
        assert.equal(lot.childNodes.includes(iframe), true);
    } finally {
        dom.cleanup();
    }
});

test('managed iframe parking-lot evicts oldest entries when over maxIframes', async () => {
    const dom = installFakeDom();
    try {
        const lotMod = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );

        const iframe1 = document.createElement('iframe');
        dom.setNowMs(0);
        lotMod.parkManagedIframe({ id: 'a', iframe: iframe1, maxIframes: 1, ttlMs: 0 });

        const iframe2 = document.createElement('iframe');
        dom.setNowMs(1);
        lotMod.parkManagedIframe({ id: 'b', iframe: iframe2, maxIframes: 1, ttlMs: 0 });

        assert.equal(lotMod.getParkedManagedIframeCount(), 1);
        assert.equal(iframe1.isConnected, false);

        assert.equal(lotMod.takeParkedManagedIframe('a'), null);
        assert.equal(lotMod.takeParkedManagedIframe('b'), iframe2);
        assert.equal(lotMod.getParkedManagedIframeCount(), 0);
    } finally {
        dom.cleanup();
    }
});

test('managed iframe parking-lot enforces TTL to avoid leaking parked iframes', async () => {
    const dom = installFakeDom();
    try {
        const lotMod = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );

        const iframe = document.createElement('iframe');
        dom.setNowMs(0);
        lotMod.parkManagedIframe({ id: 'a', iframe, maxIframes: 10, ttlMs: 10 });

        dom.setNowMs(25);
        assert.equal(lotMod.takeParkedManagedIframe('a'), null);
        assert.equal(iframe.isConnected, false);
        assert.equal(lotMod.getParkedManagedIframeCount(), 0);
    } finally {
        dom.cleanup();
    }
});

test('managed iframe parking-lot dropParkedManagedIframe hard-evicts an entry', async () => {
    const dom = installFakeDom();
    try {
        const lotMod = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );

        const iframe = document.createElement('iframe');
        lotMod.parkManagedIframe({ id: 'a', iframe, maxIframes: 10, ttlMs: 0 });
        assert.equal(lotMod.getParkedManagedIframeCount(), 1);

        lotMod.dropParkedManagedIframe('a');
        assert.equal(lotMod.getParkedManagedIframeCount(), 0);
        assert.equal(iframe.isConnected, false);
    } finally {
        dom.cleanup();
    }
});

