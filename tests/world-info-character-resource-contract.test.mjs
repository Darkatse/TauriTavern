import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const worldInfoSource = await readFile(new URL('../src/scripts/world-info.js', import.meta.url), 'utf8');
const slashCommandsSource = await readFile(new URL('../src/scripts/slash-commands.js', import.meta.url), 'utf8');
const utilsSource = await readFile(new URL('../src/scripts/utils.js', import.meta.url), 'utf8');

test('world-info rename retargets auxiliary and primary character lorebook links', () => {
    assert.match(worldInfoSource, /async function updateWorldInfoLinks\(oldName, newName\)/);
    assert.match(worldInfoSource, /await updateWorldInfoLinks\(oldName, newName\);\s*const deleted = await deleteWorldInfo\(oldName, \{ saveLinkedCharacter: false \}\)/);
    assert.match(worldInfoSource, /world_info\.charLore\?\.filter\(\(e\) => e\.extraBooks\.includes\(oldName\)\)/);
    assert.match(worldInfoSource, /\/api\/characters\/merge-attributes/);
    assert.match(worldInfoSource, /avatars: linkedCharacters\.map\(\(\{ character \}\) => character\.avatar\)/);
    assert.match(worldInfoSource, /extensions:\s*\{\s*world: newName,/);
    assert.match(worldInfoSource, /await getOneCharacter\(character\.avatar\)/);
    assert.match(worldInfoSource, /select_selected_character\(this_chid, \{ switchMenu: false \}\)/);
    assert.match(worldInfoSource, /Failed to update primary lorebook links for: \$\{failed\.join\(', '\)\}/);
});

test('world-info lorebook buttons preserve upstream click and long-press semantics', () => {
    assert.match(worldInfoSource, /export async function assignLorebookToChat\(\{ shiftKey, altKey \}\)/);
    assert.match(worldInfoSource, /if \(selectedName && !shiftKey && !altKey\)/);
    assert.match(worldInfoSource, /addLongPressEvent\('#world_button'/);
    assert.match(worldInfoSource, /addLongPressEvent\('\.chat_lorebook_button'/);
    assert.match(worldInfoSource, /#group-chat-lorebook-dropdown/);
});

test('character slash commands expose create and update avatar paths through edit-avatar', () => {
    assert.match(utilsSource, /export const supportedImageMimeTypes = Object\.freeze/);
    assert.match(utilsSource, /export async function resolveAvatarData\(input\)/);
    assert.match(slashCommandsSource, /name: 'char-create'/);
    assert.match(slashCommandsSource, /name: 'char-update'/);
    assert.match(slashCommandsSource, /name: 'char-duplicate'[\s\S]*aliases: \['dupe'\]/);
    assert.match(slashCommandsSource, /async function uploadCharacterAvatar\(avatarKey, base64Data/);
    assert.match(slashCommandsSource, /\/api\/characters\/edit-avatar/);
    assert.match(slashCommandsSource, /getRequestHeaders\(\{ omitContentType: true \}\)/);
});
