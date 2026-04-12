import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('settings popup gates data directory selection by runtime (not Bowser isMobile)', async () => {
    const popupPath = path.join(
        REPO_ROOT,
        'src/scripts/tauri/setting/setting-panel/settings-popup.js',
    );
    const source = await readFile(popupPath, 'utf8');

    assert.match(
        source,
        /import\s+\{\s*isAndroidRuntime\s*,\s*isIosRuntime\s*\}\s+from\s+['"]\.\.\/\.\.\/\.\.\/util\/mobile-runtime\.js['"]\s*;/,
    );
    assert.match(source, /const\s+supportsDataRootSelection\s*=\s*!isAndroidRuntime\(\)\s*&&\s*!isIosRuntime\(\)\s*;/);
    assert.doesNotMatch(source, /supportsDataRootSelection\s*=\s*!isMobile\(\)/);

    assert.match(source, /const\s+runtimePaths\s*=\s*supportsDataRootSelection\s*\?\s*await\s+getRuntimePaths\(\)\s*:\s*null\s*;/);
});

