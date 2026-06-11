import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function readRepoFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

async function listJsFiles(relativeDir) {
    const root = path.join(REPO_ROOT, relativeDir);
    const results = [];
    const stack = [root];

    while (stack.length > 0) {
        const current = stack.pop();
        const entries = await readdir(current, { withFileTypes: true });
        for (const entry of entries) {
            const fullPath = path.join(current, entry.name);
            if (entry.isDirectory()) {
                stack.push(fullPath);
                continue;
            }
            if (entry.isFile() && entry.name.endsWith('.js')) {
                results.push(path.relative(REPO_ROOT, fullPath).replace(/\\/g, '/'));
            }
        }
    }

    return results.sort();
}

test('TauriTavern Sync popup wrapper owns host-only capabilities', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-popup.js');

    assert.match(source, /window\.__TAURI__\?\.core\?\.invoke/);
    assert.match(source, /barcodeScanner/);
    assert.match(source, /callGenericPopup/);
    assert.match(source, /sync\.bundle\.js/);
    assert.match(source, /mountTauriTavernSyncApp/);
    assert.match(source, /parseTtSyncPairUri/);
    assert.doesNotMatch(source, /from\s+['"]vue(?:\/|['"])/);
});

test('TauriTavern Sync listeners keep event contract while delegating progress UI', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-listeners.js');

    assert.match(source, /lan_sync:progress/);
    assert.match(source, /lan_sync:completed/);
    assert.match(source, /lan_sync:error/);
    assert.match(source, /tt_sync:progress/);
    assert.match(source, /tt_sync:completed/);
    assert.match(source, /tt_sync:error/);
    assert.match(source, /mountTauriTavernSyncProgressApp/);
    assert.match(source, /window\.location\.reload\(\)/);
    assert.doesNotMatch(source, /from\s+['"]vue(?:\/|['"])/);
});

test('Rspack exposes a dedicated TauriTavern Sync Vue entry', async () => {
    const source = await readRepoFile('rspack.config.js');

    assert.match(source, /sync:\s*['"]\.\/src\/scripts\/tauri\/setting\/sync-app\/index\.js['"]/);
    assert.match(source, /listJavaScriptFiles\(['"]src\/scripts\/tauri\/setting\/sync-app['"]\)/);
    assert.match(source, /src\/scripts\/tauri\/setting\/dist/);
});

test('TauriTavern Sync Vue app stays presentation-only', async () => {
    const files = await listJsFiles('src/scripts/tauri/setting/sync-app');
    assert.ok(files.includes('src/scripts/tauri/setting/sync-app/index.js'));
    assert.ok(files.includes('src/scripts/tauri/setting/sync-app/SyncApp.js'));
    assert.ok(files.includes('src/scripts/tauri/setting/sync-app/SyncProgressApp.js'));

    const forbidden = [
        'popup.js',
        'tauri-bridge.js',
        'barcode-scanner-service.js',
        '__TAURI__',
        'lan_sync_',
        'tt_sync_',
    ];

    for (const file of files) {
        const source = await readRepoFile(file);
        for (const token of forbidden) {
            assert.doesNotMatch(source, new RegExp(token.replaceAll('.', '\\.')), `${file} contains ${token}`);
        }
    }

    const entry = await readRepoFile('src/scripts/tauri/setting/sync-app/index.js');
    assert.match(entry, /from\s+['"]vue\/dist\/vue\.esm-bundler\.js['"]/);
    assert.match(entry, /export\s+function\s+mountTauriTavernSyncApp/);
    assert.match(entry, /export\s+function\s+mountTauriTavernSyncProgressApp/);
});

test('TauriTavern Sync pure state helpers keep TT-Sync pair URI validation explicit', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-state.js');

    assert.match(source, /export\s+function\s+parseTtSyncPairUri/);
    assert.match(source, /Pair URI must start with tauritavern:\/\//);
    assert.match(source, /Pair URI is not a TT-Sync pairing link/);
    assert.match(source, /Pair URI must be v=2/);
    assert.match(source, /Pair URI missing url/);
    assert.match(source, /Pair URI missing spki/);
    assert.doesNotMatch(source, /callGenericPopup/);
    assert.doesNotMatch(source, /window\.__TAURI__/);
});
