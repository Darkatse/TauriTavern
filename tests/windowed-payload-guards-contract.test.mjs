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

test('windowed payload: windowed patch commit is guarded and preserves active cursor offset', async () => {
    const script = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const groupChats = await readFile(path.join(REPO_ROOT, 'src/scripts/group-chats.js'), 'utf8');

    assert.match(script, /\bgetWindowedChatKey\b/);
    assert.match(script, /\bmergeWindowedChatCursorOffset\b/);
    assert.match(groupChats, /\bgetWindowedChatKey\b/);
    assert.match(groupChats, /\bmergeWindowedChatCursorOffset\b/);
});
