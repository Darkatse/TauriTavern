import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function extractBetween(source, startMarker, endMarker) {
    const start = source.indexOf(startMarker);
    assert.notEqual(start, -1, `Missing marker: ${startMarker}`);
    const end = source.indexOf(endMarker, start + startMarker.length);
    assert.notEqual(end, -1, `Missing marker: ${endMarker}`);
    return source.slice(start, end);
}

test('Itemized prompts use index+record schema and avoid chat-open migration work', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/itemized-prompts.js'), 'utf8');

    assert.match(source, /tt_prompts_index:/);
    assert.match(source, /tt_prompts_record:/);

    const loadFn = extractBetween(
        source,
        'export async function loadItemizedPrompts(chatId) {',
        'export async function saveItemizedPrompts(chatId) {',
    );

    assert.match(loadFn, /await loadPromptIndex\(chatId\);/);
    assert.match(loadFn, /setActiveIndex\(chatId, \[\]\);/);
    assert.doesNotMatch(loadFn, /migrateLegacyPrompts\(chatId\)/);
    assert.doesNotMatch(loadFn, /promptStorage\.getItem\(chatId\)/);
});

test('Chat rendering and generation rely on index presence, not whole records', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    assert.match(source, /hasItemizedPromptForMessage\(messageId\)/);
    assert.match(source, /upsertItemizedPromptRecord\(additionalPromptStuff\)/);

    assert.doesNotMatch(source, /itemizedPrompts\.push\(additionalPromptStuff\)/);
});
