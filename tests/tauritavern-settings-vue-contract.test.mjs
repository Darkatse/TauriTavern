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
    assert.match(source, /onClosing:\s*async\s*\(popup\)/);
    assert.match(source, /requiresChatBackupPurgeConfirmation/);
    assert.match(source, /confirmChatBackupHistoryPurge/);
    assert.match(source, /pendingUpdate\.changes\.chatVirtualizationEnabled/);
    assert.match(source, /pendingUpdate\.next\.chatVirtualizationEnabled/);
    assert.match(source, /showChatVirtualizationCompatibility/);
    assert.match(source, /zstdCompression:\s*\{[\s\S]*existing backups are converted in the background/);
    assert.match(source, /JS-Slash-Runner 4\.9\.1 or later/);
    assert.match(source, /LittleWhiteBox 3\.0\.4 or later/);
    assert.match(source, /https:\/\/github\.com\/N0VI028\/JS-Slash-Runner/);
    assert.match(source, /https:\/\/github\.com\/RT15548\/LittleWhiteBox/);
    assert.doesNotMatch(source, /github\.com\/Darkatse\/(?:JS-Slash-Runner|LittleWhiteBox)/);
});

test('TauriTavern Settings wallpaper options use the no-render background refresh', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/setting-panel/settings-popup.js');

    assert.match(source, /refreshSystemBackgroundEntries/);
    assert.doesNotMatch(source, /getBackgrounds/);
});

test('TauriTavern Settings loads optional backup stats after opening the popup', async () => {
    const popup = await readRepoFile('src/scripts/tauri/setting/setting-panel/settings-popup.js');
    const viewModel = await readRepoFile('src/scripts/tauri/setting/setting-panel/settings-view-model.js');
    const popupIndex = popup.indexOf('const popupPromise = callTauriTavernPanelPopup');
    const statsIndex = popup.indexOf('void loadChatBackupStorageStats()');

    assert.ok(popupIndex >= 0);
    assert.ok(statsIndex > popupIndex);
    assert.match(popup, /viewModel\.values\.chatBackups\.zstdCompressionEnabled/);
    assert.match(popup, /const result = await popupPromise/);
    assert.match(viewModel, /export async function loadChatBackupStorageStats/);
    assert.doesNotMatch(
        viewModel.slice(viewModel.indexOf('export async function loadTauriTavernSettingsViewModel')),
        /getChatBackupStorageStats\(/,
    );
});

test('Rspack exposes a dedicated TauriTavern Settings Vue entry', async () => {
    const source = await readRepoFile('rspack.config.js');

    assert.match(source, /name:\s*['"]tauritavern-settings['"]/);
    assert.match(source, /settings:\s*['"]\.\/src\/scripts\/tauri\/setting\/settings-app\/index\.js['"]/);
    assert.match(source, /src\/scripts\/tauri\/setting\/dist/);
    assert.match(source, /createPersistentCache\(['"]tauritavern-settings['"],\s*tauriSettingUiCacheInputs\)/);
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
    assert.match(entry, /setChatBackupStorageStats/);

    const app = await readRepoFile('src/scripts/tauri/setting/settings-app/SettingsApp.js');
    assert.match(app, /Dynamic Theme & Wallpaper/);
    assert.match(app, /WallpaperField/);
    assert.match(app, /Chat Backups/);
    assert.match(app, /draft\.chatBackups\.zstdCompressionEnabled/);
    assert.match(app, /help-topic="zstdCompression"/);
    assert.match(app, /<br v-if="zstdCompressionHint\.saved"\s*\/\>/);
    assert.match(app, /class="tt-settings-hint-accent"/);
    assert.match(app, /formatBytes/);
    assert.match(app, /Chat DOM Virtualization/);
    assert.match(app, /help-topic="chatVirtualization"/);
    assert.match(app, /:disabled="draft\.chatVirtualizationEnabled"/);
    assert.match(app, /<ToggleSwitch v-model="draft\.chatVirtualizationEnabled"\s*\/>/);
    assert.doesNotMatch(app, /Keeps only the viewport and true tail mounted/);
    assert.doesNotMatch(app, /CHAT_SURFACE_OPTIONS|draft\.chatSurfacePolicy/);
});

test('TauriTavern Settings keeps mobile toggle rows inline', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/setting-panel/settings-app.css');

    assert.match(source, /@media\s+\(max-width:\s*640px\)/);
    assert.match(
        source,
        /\.tt-settings-row:has\(\.tt-settings-switch\)\s*\{[\s\S]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s+auto/,
    );
    assert.match(
        source,
        /\.tt-settings-row:has\(\.tt-settings-switch\)\s+\.tt-settings-control\s*\{[\s\S]*width:\s*auto[\s\S]*justify-content:\s*flex-end/,
    );
});
