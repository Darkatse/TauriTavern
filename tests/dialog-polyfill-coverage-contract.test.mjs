import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('bootstrap installs dialog polyfill coverage (main + same-origin iframes)', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/tauri/main/bootstrap.js'), 'utf8');

    assert.match(
        source,
        /import\s*\{\s*installDialogPolyfillCoverage\s*\}\s*from\s*'\.\/compat\/dialog\/dialog-polyfill-coverage\.js';/,
    );
    assert.match(source, /installDialogPolyfillCoverage\(\);/);
    assert.match(source, /installDialogPolyfillCoverage\(targetWindow\);/);
});

