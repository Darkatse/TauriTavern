import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('windowed payload: showMoreMessages implements single-flight + CAS commit', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    assert.match(source, /\bwindowedShowMoreMessagesPending\b/);
    assert.match(source, /existingPending\?\.\s*state\s*===\s*windowState/);
    assert.match(source, /getWindowedChatState\(\)\s*!==\s*windowState/);
});

test('windowed payload: showMoreMessages reindexes DOM and shifts windowed save counters after prepend', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    const showMoreStart = source.indexOf('export async function showMoreMessages');
    assert.ok(showMoreStart >= 0);
    const slice = source.slice(showMoreStart, showMoreStart + 3000);

    assert.match(slice, /updateViewMessageIds\(\s*0\s*\)\s*;/);
    assert.match(
        slice,
        /const\s+shiftedState\s*=\s*shiftWindowedMessageSaveState\(\s*windowState\s*,\s*messages\.length\s*,\s*['"]chat['"]\s*\)\s*;/,
    );
    assert.match(slice, /\.\.\.\s*shiftedState\s*,/);
});

test('windowed payload: clearChat(clearData:true) invalidates windowed state immediately', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    const clearChatStart = source.indexOf('export async function clearChat');
    assert.ok(clearChatStart >= 0);
    const slice = source.slice(clearChatStart, clearChatStart + 600);

    assert.match(slice, /cancelDebouncedMetadataSave\(\);/);
    assert.match(slice, /if\s*\(\s*clearData\s*\)\s*\{\s*clearWindowedChatState\(\);\s*\}/s);
});

test('windowed payload: getChat drops stale tail-load results before committing UI/state', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    const getChatStart = source.indexOf('export async function getChat()');
    assert.ok(getChatStart >= 0);
    const slice = source.slice(getChatStart, getChatStart + 3200);

    assert.match(slice, /const\s+startedChid\s*=\s*this_chid\s*;/);
    assert.match(slice, /\bconst\s+stillActive\s*=\s*[\s\S]*?;\s*if\s*\(!stillActive\)\s*\{\s*return;\s*\}/s);
});

test('windowed payload: group tail-load does not mutate window state for background reads', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/group-chats.js'), 'utf8');

    assert.match(source, /async function loadGroupChat\(\s*chatId\s*,\s*\{\s*updateWindowState\s*=\s*false\s*\}\s*=\s*\{\s*\}\s*\)/);
    assert.match(source, /if\s*\(\s*updateWindowState[\s\S]*?setWindowedChatState\s*\(/s);
});

test('windowed payload: windowed patch commit is guarded and merges cursor offsets', async () => {
    const script = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const groupChats = await readFile(path.join(REPO_ROOT, 'src/scripts/group-chats.js'), 'utf8');

    const saveChatStart = script.indexOf('export async function saveChat');
    assert.ok(saveChatStart >= 0);
    const saveChatSlice = script.slice(saveChatStart, saveChatStart + 3200);

    assert.match(saveChatSlice, /\bgetWindowedChatKey\b/);
    assert.match(saveChatSlice, /const\s+expectedCursorOffset\s*=\s*windowState\.cursor\.offset\s*;/);
    assert.match(
        saveChatSlice,
        /mergeWindowedChatCursorOffset\(\s*activeWindowState\?\.\s*cursor\s*,\s*cursor\s*,\s*expectedCursorOffset\s*\)/,
    );
    assert.match(saveChatSlice, /activeWindowState\?\.\s*cursor\?\.\s*offset\s*===\s*expectedCursorOffset/);

    const saveGroupStart = groupChats.indexOf('async function saveGroupChat');
    assert.ok(saveGroupStart >= 0);
    const saveGroupSlice = groupChats.slice(saveGroupStart, saveGroupStart + 2400);

    assert.match(saveGroupSlice, /\bgetWindowedChatKey\b/);
    assert.match(saveGroupSlice, /const\s+expectedCursorOffset\s*=\s*windowState\.cursor\.offset\s*;/);
    assert.match(
        saveGroupSlice,
        /mergeWindowedChatCursorOffset\(\s*activeWindowState\?\.\s*cursor\s*,\s*cursor\s*,\s*expectedCursorOffset\s*\)/,
    );
    assert.match(saveGroupSlice, /activeWindowState\?\.\s*cursor\?\.\s*offset\s*===\s*expectedCursorOffset/);
});
