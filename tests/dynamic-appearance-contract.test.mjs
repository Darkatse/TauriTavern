import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function readRepoFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('dynamic appearance runtime applies SillyTavern appearance through the ST adapter', async () => {
    const runtime = await readRepoFile('src/tauri/main/services/dynamic-theme/install.js');
    const adapter = await readRepoFile('src/tauri/main/adapters/st/appearance.js');

    assert.match(runtime, /applySillyTavernTheme/);
    assert.match(runtime, /applySillyTavernGlobalBackground/);
    assert.doesNotMatch(runtime, /scripts\/backgrounds\.js/);
    assert.match(adapter, /import\('\.\.\/\.\.\/\.\.\/\.\.\/scripts\/backgrounds\.js'\)/);
});

test('dynamic appearance runtime validates targets before applying appearance', async () => {
    const runtime = await readRepoFile('src/tauri/main/services/dynamic-theme/install.js');
    const themeAssert = runtime.indexOf('assertSillyTavernThemeAvailable(targetTheme)');
    const wallpaperAssert = runtime.indexOf('await assertSillyTavernGlobalBackgroundAvailable(targetWallpaper)');
    const themeApply = runtime.indexOf('applySillyTavernTheme(targetTheme)');
    const wallpaperApply = runtime.indexOf('await applySillyTavernGlobalBackground(targetWallpaper)');

    assert.ok(themeAssert > -1);
    assert.ok(wallpaperAssert > -1);
    assert.ok(themeApply > -1);
    assert.ok(wallpaperApply > -1);
    assert.ok(themeAssert < themeApply);
    assert.ok(wallpaperAssert < wallpaperApply);
});

test('global background helper reuses background state instead of simulating UI clicks', async () => {
    const source = await readRepoFile('src/scripts/backgrounds.js');
    const start = source.indexOf('export async function applyGlobalBackground');
    const end = source.indexOf('async function delBackground', start);

    assert.notEqual(start, -1);
    assert.notEqual(end, -1);
    const helper = source.slice(start, end);
    assert.match(helper, /assertSystemBackgroundExists/);
    assert.match(helper, /setBackground/);
    assert.match(helper, /highlightSelectedBackground/);
    assert.doesNotMatch(helper, /\.click\(/);
});

test('background option refresh is separated from drawer rendering', async () => {
    const source = await readRepoFile('src/scripts/backgrounds.js');
    const refreshStart = source.indexOf('export async function refreshSystemBackgroundEntries');
    const getBackgroundsStart = source.indexOf('export async function getBackgrounds');
    const getBackgroundsEnd = source.indexOf('/**\n * Preloads all image metadata', getBackgroundsStart);

    assert.notEqual(refreshStart, -1);
    assert.notEqual(getBackgroundsStart, -1);
    assert.notEqual(getBackgroundsEnd, -1);

    const refresh = source.slice(refreshStart, getBackgroundsStart);
    const getBackgrounds = source.slice(getBackgroundsStart, getBackgroundsEnd);
    assert.doesNotMatch(refresh, /renderSystemBackgrounds|loadFolders|preloadImageMetadata/);
    assert.match(getBackgrounds, /refreshSystemBackgroundEntries/);
    assert.match(getBackgrounds, /renderSystemBackgrounds/);
});
