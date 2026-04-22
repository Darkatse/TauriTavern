import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('resetScrollHeight avoids transient zero-height collapse', async () => {
    const utilsPath = path.join(REPO_ROOT, 'src/scripts/utils.js');
    const source = await readFile(utilsPath, 'utf8');

    const match = source.match(
        /export async function resetScrollHeight\(element\)\s*\{([\s\S]*?)\n\}/,
    );
    assert.ok(match, 'resetScrollHeight implementation not found');

    const body = match[1];
    assert.doesNotMatch(body, /css\('height',\s*'0px'\)/);
    assert.match(body, /css\('height',\s*'auto'\)/);
});

