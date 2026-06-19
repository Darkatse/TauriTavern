import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('hasAnimation ignores reduced-motion zero-duration animations', async () => {
    const utilsPath = path.join(REPO_ROOT, 'src/scripts/utils.js');
    const source = await readFile(utilsPath, 'utf8');

    const match = source.match(
        /export function hasAnimation\(control\)\s*\{([\s\S]*?)\n\}/,
    );
    assert.ok(match, 'hasAnimation implementation not found');

    const body = match[1];
    assert.match(body, /animationName/);
    assert.match(body, /animationDuration/);
    assert.match(body, /parseCssTimeMilliseconds\(duration\)\s*>\s*0/);
});
