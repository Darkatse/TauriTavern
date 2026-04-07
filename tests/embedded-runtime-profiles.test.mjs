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

test('embedded-runtime profiles: auto resolves to mobile-safe on mobile user agents', async () => {
    const dom = installFakeDom({ userAgent: 'Mozilla/5.0 (Android 14) AppleWebKit/537.36', platform: 'Linux armv8l' });
    try {
        const { resolveEmbeddedRuntimeProfile } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-profiles.js'),
        );

        const profile = resolveEmbeddedRuntimeProfile('auto');
        assert.equal(profile.name, 'mobile-safe');
        assert.equal(profile.maxActiveIframes, 4);
        assert.equal(profile.maxSoftParkedIframes, 8);
    } finally {
        dom.cleanup();
    }
});

test('embedded-runtime profiles: auto resolves to mobile-safe for iPad masquerading as Mac', async () => {
    const dom = installFakeDom({ userAgent: 'Mozilla/5.0 (Macintosh) AppleWebKit/605.1.15', platform: 'MacIntel', maxTouchPoints: 5 });
    try {
        const { resolveEmbeddedRuntimeProfile } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-profiles.js'),
        );

        const profile = resolveEmbeddedRuntimeProfile('auto');
        assert.equal(profile.name, 'mobile-safe');
    } finally {
        dom.cleanup();
    }
});

test('embedded-runtime profiles: auto resolves to compat on desktop', async () => {
    const dom = installFakeDom({ userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)', platform: 'Win32' });
    try {
        const { resolveEmbeddedRuntimeProfile } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-profiles.js'),
        );

        const profile = resolveEmbeddedRuntimeProfile('auto');
        assert.equal(profile.name, 'compat');
        assert.equal(profile.maxActiveIframes, 12);
        assert.equal(profile.maxSoftParkedIframes, 24);
    } finally {
        dom.cleanup();
    }
});

test('embedded-runtime profiles expose parkWhenHiddenKinds and reject off', async () => {
    const dom = installFakeDom();
    try {
        const { resolveEmbeddedRuntimeProfile } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-profiles.js'),
        );
        const { EmbeddedRuntimeKind } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/runtime-kinds.js'),
        );

        const profile = resolveEmbeddedRuntimeProfile('compat');
        assert.ok(profile.parkWhenHiddenKinds.includes(EmbeddedRuntimeKind.JsrHtmlRender));
        assert.ok(profile.parkWhenHiddenKinds.includes(EmbeddedRuntimeKind.LittleWhiteBoxHtmlRender));

        assert.throws(() => resolveEmbeddedRuntimeProfile('off'), /does not resolve/);
    } finally {
        dom.cleanup();
    }
});

