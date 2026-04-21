import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('bootstrap installs mobile runtime compat in same-origin iframes', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/tauri/main/bootstrap.js'), 'utf8');

    assert.match(source, /runtimeCompat:\s*isMobile\s*\?\s*installMobileRuntimeCompat\s*:\s*null/);
    assert.match(source, /runtimeCompat\?\.\(targetWindow\);/);
});

