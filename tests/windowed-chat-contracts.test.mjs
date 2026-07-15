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

test('full-DOM chat progressively contains inactive message roots', async () => {
    const style = await readFile(path.join(REPO_ROOT, 'src/style.css'), 'utf8');

    assert.match(
        style,
        /@supports\s*\(content-visibility:\s*auto\)\s*and\s*\(contain-intrinsic-block-size:\s*auto 200px\)/,
    );
    assert.match(
        style,
        /#chat\s*>\s*\.mes:not\(\.last_mes\)\s*\{\s*content-visibility:\s*auto;\s*contain-intrinsic-block-size:\s*auto 200px;\s*flex-shrink:\s*0;/,
    );
    assert.match(style, /\.auto_hide\s*\{\s*content-visibility:\s*auto;/);
});

test('chat history off preserves the upstream full-data and truncated-DOM rendering baseline', async () => {
    const script = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const groupChats = await readFile(path.join(REPO_ROOT, 'src/scripts/group-chats.js'), 'utf8');
    const transport = await readFile(path.join(REPO_ROOT, 'src/scripts/tauri/chat/transport.js'), 'utf8');

    assert.match(
        transport,
        /return\s+getChatHistoryBootstrapModeName\(\)\s*===\s*CHAT_HISTORY_MODE_WINDOWED\s*;/,
    );

    const getChat = script.slice(
        script.indexOf('export async function getChat'),
        script.indexOf('\nasync function getChatResult'),
    );
    assert.match(getChat, /if\s*\(usePayloadTransport\)[\s\S]*?loadCharacterChatPayloadTail\s*\(/);
    assert.match(getChat, /else\s*\{\s*clearWindowedChatState\(\);[\s\S]*?fetch\(['"]\/api\/chats\/get['"]/);
    assert.match(getChat, /data\s*=\s*await response\.json\(\)\s*;/);

    const loadGroupChat = groupChats.slice(
        groupChats.indexOf('async function loadGroupChat'),
        groupChats.indexOf('\nasync function hasPersistedGroupChats'),
    );
    assert.match(loadGroupChat, /if\s*\(isTauriChatPayloadTransportEnabled\(\)\)[\s\S]*?loadGroupChatPayloadTail\s*\(/);
    assert.match(loadGroupChat, /clearWindowedChatState\(\);[\s\S]*?fetch\(['"]\/api\/chats\/group\/get['"]/);
    assert.match(loadGroupChat, /return\s+data\s*;/);

    const printMessages = script.slice(
        script.indexOf('export async function printMessages'),
        script.indexOf('\n/**', script.indexOf('export async function printMessages')),
    );
    assert.match(printMessages, /const\s+count\s*=\s*power_user\.chat_truncation\s*\|\|\s*Number\.MAX_SAFE_INTEGER\s*;/);
    assert.match(printMessages, /startIndex\s*=\s*chat\.length\s*-\s*count\s*;/);
    assert.match(printMessages, /id=["']show_more_messages["']/);

    const showMoreMessages = script.slice(
        script.indexOf('export async function showMoreMessages'),
        script.indexOf('\nexport async function printMessages'),
    );
    const upstreamShowMore = showMoreMessages.slice(showMoreMessages.indexOf('    const firstDisplayedMesId'));
    assert.match(upstreamShowMore, /chat\.slice\(firstId,\s*messageId\)\.forEach\s*\(/);
    assert.match(upstreamShowMore, /showMoreButton\.after\(messageElements\)\s*;/);
    assert.match(upstreamShowMore, /chatElement\.prepend\(messageElements\)\s*;/);
    assert.doesNotMatch(upstreamShowMore, /loadCharacterChatPayloadBefore|createDocumentFragment|applyCharacterTagsToMessageDivs/);

    const redisplayChat = script.slice(
        script.indexOf('export async function redisplayChat'),
        script.indexOf('\nexport function scrollOnMediaLoad'),
    );
    assert.match(redisplayChat, /const\s+messages\s*=\s*targetChat\.slice\(startIndex\)\s*;/);
    assert.match(redisplayChat, /const\s+newMessageElements\s*=\s*messages\.map\s*\(/);
    assert.match(redisplayChat, /chatElement\.append\(newMessageElements\)\s*;/);
    assert.doesNotMatch(redisplayChat, /requestAnimationFrame|batchSize|appendTarget/);
});
