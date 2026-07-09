import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function sliceSource(source, start, end) {
    const startIndex = source.indexOf(start);
    assert.notEqual(startIndex, -1, `Missing source marker: ${start}`);
    const endIndex = source.indexOf(end, startIndex);
    assert.notEqual(endIndex, -1, `Missing source marker: ${end}`);
    return source.slice(startIndex, endIndex);
}

test('avatar mutations reload the canonical thumbnail URL', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const avatarEditSource = sliceSource(
        source,
        'async function read_avatar_load(input)',
        'export function getThumbnailUrl',
    );
    const importCharacterSource = sliceSource(
        source,
        'async function importCharacter(file',
        'async function importFromURL',
    );

    assert.match(
        avatarEditSource,
        /const thumbnailUrl = getThumbnailUrl\('avatar', avatarKey\);\s*await fetch\(thumbnailUrl, \{ method: 'GET', cache: 'reload' \}\);/,
    );
    assert.doesNotMatch(avatarEditSource, /getThumbnailUrl\('avatar', avatarKey,\s*true\)/);
    assert.match(
        importCharacterSource,
        /await fetch\(getThumbnailUrl\('avatar', avatarFileName\), \{ cache: 'reload' \}\);/,
    );
    assert.doesNotMatch(importCharacterSource, /getThumbnailUrl\('avatar', avatarFileName,\s*true\)/);
});
