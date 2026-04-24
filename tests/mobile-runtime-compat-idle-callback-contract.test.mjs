import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('Mobile runtime compat polyfills requestIdleCallback', async () => {
    const source = await readFile(
        path.join(REPO_ROOT, 'src/tauri/main/compat/mobile/mobile-runtime-compat.js'),
        'utf8',
    );

    assert.match(source, /export function installMobileRuntimeCompat\(targetWindow = window\)/);
    assert.match(source, /defineMissingGlobalMethod\(\s*targetWindow,\s*['"]requestIdleCallback['"]/);
    assert.match(source, /defineMissingGlobalMethod\(\s*targetWindow,\s*['"]cancelIdleCallback['"]/);

    assert.match(source, /\bfunction createRequestIdleCallbackPolyfill\b/);
    assert.match(source, /didTimeout:/);
    assert.match(source, /timeRemaining:/);
    assert.match(source, /requestIdleCallback: callback must be a function/);
});
