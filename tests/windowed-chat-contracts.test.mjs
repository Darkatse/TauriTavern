import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('windowed chat contract: all chat save entrypoints are serialized (enqueueChatSave)', async () => {
    const script = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const groupChats = await readFile(path.join(REPO_ROOT, 'src/scripts/group-chats.js'), 'utf8');

    assert.match(script, /export function enqueueChatSave\s*\(/);

    const saveChatStart = script.indexOf('export async function saveChat');
    assert.ok(saveChatStart >= 0);
    const saveChatSlice = script.slice(saveChatStart, saveChatStart + 500);
    assert.match(saveChatSlice, /return\s+enqueueChatSave\s*\(/);

    const conditionalStart = script.indexOf('export async function saveChatConditional');
    assert.ok(conditionalStart >= 0);
    const conditionalSlice = script.slice(conditionalStart, conditionalStart + 800);
    assert.match(conditionalSlice, /enqueueChatSave\s*\(/);
    assert.doesNotMatch(conditionalSlice, /waitUntilCondition\s*\(\s*\(\s*\)\s*=>\s*!isChatSaving/);

    const saveGroupStart = groupChats.indexOf('async function saveGroupChat');
    assert.ok(saveGroupStart >= 0);
    const saveGroupSlice = groupChats.slice(saveGroupStart, saveGroupStart + 300);
    assert.match(saveGroupSlice, /return\s+enqueueChatSave\s*\(/);

    assert.match(script, /export\s+let\s+isChatSaving\s*=\s*false\s*;/);
    const literalAssignments = script.match(/\bisChatSaving\s*=\s*(true|false)\b/g) ?? [];
    assert.deepEqual(literalAssignments, ['isChatSaving = false']);
});

test('windowed chat contract: UI + prompt-backfill share window-size defaults', async () => {
    const windowedState = await readFile(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'), 'utf8');
    const promptBackfill = await readFile(path.join(REPO_ROOT, 'src/scripts/tauri/chat/prompt-backfill.js'), 'utf8');

    assert.match(windowedState, /from\s+['"]\.\/windowed-defaults\.js['"]/);
    assert.match(promptBackfill, /from\s+['"]\.\/windowed-defaults\.js['"]/);
    assert.match(promptBackfill, /\bDEFAULT_CHAT_WINDOW_LINES_MOBILE\b/);
    assert.match(promptBackfill, /\bDEFAULT_CHAT_WINDOW_LINES_DESKTOP\b/);
});

test('windowed chat contract: cursor signature normalizes modifiedMillis and modified_millis', async () => {
    const promptBackfill = await readFile(path.join(REPO_ROOT, 'src/scripts/tauri/chat/prompt-backfill.js'), 'utf8');
    assert.match(promptBackfill, /modifiedMillis\s*\?\?\s*cursor\?\.\s*modified_millis/);
});
