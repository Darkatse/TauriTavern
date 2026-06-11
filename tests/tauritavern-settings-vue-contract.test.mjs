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

test('TauriTavern Settings popup is a host wrapper around the Vue bundle', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/setting-panel/settings-popup.js');

    assert.match(source, /importSettingsBundle/);
    assert.match(source, /\.\.\/dist\/settings\.bundle\.js/);
    assert.match(source, /mountTauriTavernSettingsApp/);
    assert.doesNotMatch(source, /from\s+['"]vue(?:\/|['"])/);
    assert.match(source, /buildTauriTavernSettingsUpdate\(viewModel\.values,\s*appHandle\.getDraft\(\)\)/);
    assert.match(source, /applyTauriTavernSettingsUpdateEffects\(update,\s*updatedSettings\)/);
});

test('webpack exposes a dedicated TauriTavern Settings Vue entry', async () => {
    const source = await readRepoFile('webpack.config.js');

    assert.match(source, /name:\s*['"]tauritavern-settings['"]/);
    assert.match(source, /settings:\s*['"]\.\/src\/scripts\/tauri\/setting\/settings-app\/index\.js['"]/);
    assert.match(source, /src\/scripts\/tauri\/setting\/dist/);
    assert.match(source, /createFilesystemCache\(['"]tauritavern-settings['"],\s*tauriSettingUiCacheInputs\)/);
    assert.match(source, /createVueDefinePlugin\(\)/);
});

test('TauriTavern Settings Vue app stays presentation-only', async () => {
    const files = await listJsFiles('src/scripts/tauri/setting/settings-app');
    assert.ok(files.includes('src/scripts/tauri/setting/settings-app/index.js'));
    assert.ok(files.includes('src/scripts/tauri/setting/settings-app/SettingsApp.js'));

    const forbiddenImports = [
        'popup.js',
        'tauri-bridge.js',
        'dev-logs.js',
        'sync-popup.js',
    ];

    for (const file of files) {
        const source = await readRepoFile(file);
        for (const forbidden of forbiddenImports) {
            assert.doesNotMatch(source, new RegExp(forbidden.replace('.', '\\.')), `${file} imports ${forbidden}`);
        }
    }

    const entry = await readRepoFile('src/scripts/tauri/setting/settings-app/index.js');
    assert.match(entry, /from\s+['"]vue\/dist\/vue\.esm-bundler\.js['"]/);
    assert.match(entry, /export\s+function\s+mountTauriTavernSettingsApp/);
});
