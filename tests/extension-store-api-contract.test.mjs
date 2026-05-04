import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(relativePath) {
    const modulePath = path.join(REPO_ROOT, relativePath);
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

test('extension store tryGetJson uses optional lookup command', async () => {
    const calls = [];
    globalThis.window = { __TAURITAVERN__: { api: {} } };

    const { installExtensionStoreApi } = await importFresh('src/tauri/main/api/extension-store.js');
    installExtensionStoreApi({
        async safeInvoke(command, args) {
            calls.push({ command, args });
            return { found: false };
        },
    });

    const result = await window.__TAURITAVERN__.api.extension.store.tryGetJson({
        namespace: 'my-ext',
        table: 'state',
        key: 'settings',
    });

    assert.deepEqual(result, { found: false });
    assert.deepEqual(calls, [{
        command: 'try_get_extension_store_json',
        args: {
            namespace: 'my-ext',
            table: 'state',
            key: 'settings',
        },
    }]);
});
