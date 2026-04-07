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

test('EmbeddedRuntimeService fails fast when #chat root is missing', async () => {
    const dom = installFakeDom();
    try {
        const { createEmbeddedRuntimeService } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-service.js'),
        );

        assert.throws(
            () => createEmbeddedRuntimeService({ profileName: 'compat' }),
            /#chat root not found/,
        );
    } finally {
        dom.cleanup();
    }
});

test('EmbeddedRuntimeService exposes profile+manager and publishes a global handle', async () => {
    const dom = installFakeDom();
    try {
        const { createEmbeddedRuntimeService } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/embedded-runtime-service.js'),
        );

        const chat = document.createElement('div');
        chat.setAttribute('id', 'chat');
        document.body.append(chat);

        const service = createEmbeddedRuntimeService({ profileName: 'compat' });
        assert.equal(service.profile.name, 'compat');
        assert.equal(service.manager.profile, 'compat');
        assert.equal(globalThis.__TAURITAVERN_EMBEDDED_RUNTIME__, service.manager);
    } finally {
        delete globalThis.__TAURITAVERN_EMBEDDED_RUNTIME__;
        dom.cleanup();
    }
});

