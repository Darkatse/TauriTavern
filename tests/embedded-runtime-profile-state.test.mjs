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

test('profile-state normalizes supported profile names and fails fast on unsupported', async () => {
    const dom = installFakeDom();
    try {
        const mod = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js'),
        );

        assert.equal(mod.normalizeEmbeddedRuntimeProfileName('auto'), 'auto');
        assert.equal(mod.normalizeEmbeddedRuntimeProfileName('off'), 'off');
        assert.equal(mod.normalizeEmbeddedRuntimeProfileName('compat'), 'compat');
        assert.equal(mod.normalizeEmbeddedRuntimeProfileName('mobile-safe'), 'mobile-safe');
        assert.throws(() => mod.normalizeEmbeddedRuntimeProfileName(''), /required/);
        assert.throws(() => mod.normalizeEmbeddedRuntimeProfileName('nope'), /Unsupported embedded runtime profile/);
    } finally {
        dom.cleanup();
    }
});

test('profile-state resolves bootstrap profile from stored, else legacy, else auto', async () => {
    const dom = installFakeDom();
    try {
        globalThis.localStorage.setItem('tt:runtimeProfile', 'compat');

        const mod = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js'),
        );

        assert.equal(mod.getEmbeddedRuntimeBootstrapProfileName(), 'compat');

        mod.setEmbeddedRuntimeBootstrapProfileName('mobile-safe');
        assert.equal(globalThis.localStorage.getItem('tt:embeddedRuntimeProfile'), 'mobile-safe');
        assert.equal(mod.getEmbeddedRuntimeBootstrapProfileName(), 'mobile-safe');

        mod.clearLegacyEmbeddedRuntimeProfileName();
        assert.equal(globalThis.localStorage.getItem('tt:runtimeProfile'), null);
    } finally {
        dom.cleanup();
    }
});

test('profile-state effective profile respects legacy only when no new stored key exists', async () => {
    const dom = installFakeDom();
    try {
        // Legacy exists, no new key -> auto resolves to legacy.
        globalThis.localStorage.setItem('tt:runtimeProfile', 'compat');
        const mod1 = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js'),
        );
        assert.equal(mod1.resolveEffectiveEmbeddedRuntimeProfileName('auto'), 'compat');

        // New key exists -> auto stays auto (explicit choice), legacy ignored.
        globalThis.localStorage.setItem('tt:embeddedRuntimeProfile', 'auto');
        const mod2 = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js'),
        );
        assert.equal(mod2.resolveEffectiveEmbeddedRuntimeProfileName('auto'), 'auto');
    } finally {
        dom.cleanup();
    }
});

test('profile-state exposes a fast off switch for embedded-runtime takeover', async () => {
    const dom = installFakeDom();
    try {
        const mod = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js'),
        );
        mod.setEmbeddedRuntimeBootstrapProfileName('off');
        assert.equal(mod.isEmbeddedRuntimeTakeoverDisabled(), true);
    } finally {
        dom.cleanup();
    }
});

