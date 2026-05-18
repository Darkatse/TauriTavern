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
        'export async function saveItemizedPrompts(chatId,',
    );

    assert.match(loadFn, /await loadPromptIndex\(chatId\);/);
    assert.match(loadFn, /setActiveIndex\(chatId, \[\]\);/);
    assert.doesNotMatch(loadFn, /migrateLegacyPrompts\(chatId\)/);
    assert.doesNotMatch(loadFn, /promptStorage\.getItem\(chatId\)/);
});

test('Itemized prompt lifecycle events wait for durable storage boundaries', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/itemized-prompts.js'), 'utf8');
    const saveFn = extractBetween(
        source,
        'export async function saveItemizedPrompts(chatId,',
        '/**\n * Replaces the itemized prompt text for a message.',
    );
    const deleteFn = extractBetween(
        source,
        'export async function deleteItemizedPrompts(chatId) {',
        '/**\n * Empties the itemized prompts array and caches.',
    );
    const clearFn = extractBetween(
        source,
        'export async function clearItemizedPrompts() {',
        'export function unloadItemizedPrompts() {',
    );

    assert.match(source, /async function flushPendingWritesDurable\(\)/);
    assert.match(source, /const pendingIndexWrites = new Map\(\);/);
    assert.match(source, /while \(pendingIndexWrites\.size > 0 && pendingRecordWrites\.size === 0\)/);

    assert.match(saveFn, /requestIndexWrite\(chatId, entriesSnapshot\);\s*await flushPendingWritesDurable\(\);/);
    assert.match(saveFn, /requestIndexWrite\(activeChatId, itemizedPrompts\);\s*await flushPendingWritesDurable\(\);/);
    assert.match(saveFn, /await flushPendingWritesDurable\(\);[\s\S]*ITEMIZED_PROMPTS_SAVED/);

    assert.match(deleteFn, /cancelPendingWritesForChat\(chatId\);\s*await waitForActiveFlush\(\);/);
    assert.match(deleteFn, /await waitForActiveFlush\(\);[\s\S]*promptStorage\.keys\(\)/);
    assert.match(deleteFn, /promptStorage\.removeItem\(key\)[\s\S]*ITEMIZED_PROMPTS_DELETED/);

    assert.match(clearFn, /pendingRecordWrites\.clear\(\);\s*pendingIndexWrites\.clear\(\);\s*await waitForActiveFlush\(\);\s*await promptStorage\.clear\(\);/);
    assert.match(clearFn, /await promptStorage\.clear\(\);[\s\S]*ITEMIZED_PROMPTS_DELETED/);
});

test('Chat rendering and generation rely on index presence, not whole records', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const saveConditionalFn = extractBetween(
        source,
        'export async function saveChatConditional() {',
        '/**\n * Saves the chat to the server.',
    );

    assert.match(source, /hasItemizedPromptForMessage\(messageId\)/);
    assert.match(source, /upsertItemizedPromptRecord\(additionalPromptStuff\)/);
    assert.match(saveConditionalFn, /const chatId = getCurrentChatId\(\);\s*const tokenCacheSaveState = captureTokenCacheSaveState\(chatId\);\s*const itemizedPromptsSnapshot = captureItemizedPromptsSaveSnapshot\(chatId\);/);
    assert.match(saveConditionalFn, /enqueueChatSave\(async \(\) => \{\s*await saveTokenCache\(tokenCacheSaveState\);/);
    assert.match(saveConditionalFn, /await saveItemizedPrompts\(chatId,\s*\{\s*entriesSnapshot: itemizedPromptsSnapshot,\s*cloneFromActive: false,\s*\}\);/);
    assert.doesNotMatch(saveConditionalFn, /saveItemizedPrompts\(getCurrentChatId\(\)\)/);

    assert.doesNotMatch(source, /itemizedPrompts\.push\(additionalPromptStuff\)/);
});

test('Itemized prompt public lifecycle functions reject directly on storage failures', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/itemized-prompts.js'), 'utf8');
    const loadFn = extractBetween(
        source,
        'export async function loadItemizedPrompts(chatId) {',
        'export async function saveItemizedPrompts(chatId,',
    );
    const saveFn = extractBetween(
        source,
        'export async function saveItemizedPrompts(chatId,',
        '/**\n * Replaces the itemized prompt text for a message.',
    );
    const deleteFn = extractBetween(
        source,
        'export async function deleteItemizedPrompts(chatId) {',
        '/**\n * Empties the itemized prompts array and caches.',
    );
    const clearFn = extractBetween(
        source,
        'export async function clearItemizedPrompts() {',
        'export function unloadItemizedPrompts() {',
    );

    for (const fn of [loadFn, saveFn, deleteFn, clearFn]) {
        assert.doesNotMatch(fn, /queueMicrotask/);
        assert.match(fn, /catch \(error\) \{[\s\S]*throw error;/);
    }
});
