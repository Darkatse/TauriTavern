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

test('avatar mutations re-demand canonical Host URLs through image consumers', async () => {
    const [source, slashCommandsSource] = await Promise.all([
        readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/scripts/slash-commands.js'), 'utf8'),
    ]);
    const avatarEditSource = sliceSource(
        source,
        'async function read_avatar_load(input)',
        'export function getThumbnailUrl',
    );
    const refreshSource = sliceSource(
        source,
        'export async function refreshCharacterAvatarImages(avatarKey)',
        'export function buildAvatarList',
    );
    const importCharacterSource = sliceSource(
        source,
        'async function importCharacter(file',
        'async function importFromURL',
    );
    const slashAvatarSource = sliceSource(
        slashCommandsSource,
        'async function uploadCharacterAvatar(avatarKey',
        'async function createCharacterCallback',
    );

    assert.match(avatarEditSource, /await refreshCharacterAvatarImages\(avatarKey\);/);
    assert.match(importCharacterSource, /await refreshCharacterAvatarImages\(avatarFileName\);/);
    assert.match(slashAvatarSource, /await refreshCharacterAvatarImages\(avatarKey\);/);
    for (const mutationSource of [avatarEditSource, importCharacterSource, slashAvatarSource]) {
        assert.doesNotMatch(mutationSource, /cache\s*:\s*['"]reload['"]/);
    }

    assert.match(refreshSource, /getThumbnailUrl\('avatar', avatarKey\)/);
    assert.match(refreshSource, /`\/characters\/\$\{encodeURIComponent\(avatarKey\)\}`/);
    assert.match(refreshSource, /image\.removeAttribute\('src'\)/);
    assert.match(refreshSource, /requestAnimationFrame/);
    assert.match(refreshSource, /image\.src = src/);
});
