import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/**
 * Extract a function body by brace-balancing from a header marker, so a guard
 * assertion can't be silently defeated by the function growing past a fixed
 * character window. Returns the slice from the marker through the matching
 * closing brace of the first `{` after it.
 */
function extractFunctionBody(source, marker) {
    const start = source.indexOf(marker);
    assert.ok(start >= 0, `marker not found: ${marker}`);

    // Skip the parameter list first — a destructured signature like
    // `fn({ a, b } = {})` has braces that would otherwise be matched as the
    // body. Balance the signature parentheses to find where params end.
    const paramOpen = source.indexOf('(', start);
    assert.ok(paramOpen >= 0, `no parameter list after: ${marker}`);
    let parenDepth = 0;
    let paramEnd = -1;
    for (let i = paramOpen; i < source.length; i += 1) {
        if (source[i] === '(') parenDepth += 1;
        else if (source[i] === ')') {
            parenDepth -= 1;
            if (parenDepth === 0) { paramEnd = i; break; }
        }
    }
    assert.ok(paramEnd >= 0, `unbalanced parameter list: ${marker}`);

    const open = source.indexOf('{', paramEnd);
    assert.ok(open >= 0, `no function body after: ${marker}`);
    let depth = 0;
    for (let i = open; i < source.length; i += 1) {
        const ch = source[i];
        if (ch === '{') depth += 1;
        else if (ch === '}') {
            depth -= 1;
            if (depth === 0) return source.slice(start, i + 1);
        }
    }
    throw new Error(`unbalanced braces from marker: ${marker}`);
}

test('windowed payload: showMoreMessages implements stable-key single-flight + stale-load guard', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    assert.match(source, /\bwindowedShowMoreMessagesPending\b/);
    assert.match(source, /existingPending\?\.\s*key\s*===\s*windowKey/);
    assert.match(source, /getWindowedChatState\(\)\s*!==\s*windowState/);
});

test('windowed payload: showMoreMessages commits prepend save state before yielding to DOM rendering', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    const slice = extractFunctionBody(source, 'export async function showMoreMessages');
    const prependIndex = slice.indexOf('chat.splice(0, 0, ...messages)');
    const shiftIndex = slice.indexOf('shiftWindowedMessageSaveState', prependIndex);
    const stateCommitIndex = slice.indexOf('setWindowedChatState({', shiftIndex);
    const renderIndex = slice.indexOf('await prependWindowedMessageElements', stateCommitIndex);

    assert.ok(prependIndex >= 0, 'chat prepend mutation not found');
    assert.ok(shiftIndex > prependIndex, 'save counters must shift after the chat prepend');
    assert.ok(stateCommitIndex > shiftIndex, 'shifted state must be committed');
    assert.ok(renderIndex > stateCommitIndex, 'window state must be committed before the first render yield');
    assert.match(slice, /updateViewMessageIds\(\s*0\s*\)\s*;/);
    assert.match(
        slice,
        /const\s+shiftedState\s*=\s*shiftWindowedMessageSaveState\(\s*windowState\s*,\s*messages\.length\s*,\s*['"]chat['"]\s*\)\s*;/,
    );
    assert.match(slice, /\.\.\.\s*shiftedState\s*,/);
    assert.doesNotMatch(slice.slice(renderIndex), /shiftWindowedMessageSaveState|setWindowedChatState/);
});

test('windowed payload: prepend batches use local messages and stop after a chat switch', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const slice = extractFunctionBody(source, 'async function prependWindowedMessageElements');

    assert.match(slice, /updateMessageElement\(\s*messages\[id\]/);
    assert.doesNotMatch(slice, /updateMessageElement\(\s*chat\[id\]/);
    assert.match(slice, /if\s*\(\s*!isCurrentWindow\(\)\s*\)\s*\{\s*return false;/s);
    assert.match(slice, /return isCurrentWindow\(\);/);
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

    const getChatStart = source.indexOf('export async function getChat');
    assert.ok(getChatStart >= 0);
    const slice = source.slice(getChatStart, getChatStart + 3200);

    assert.match(slice, /const\s+startedChid\s*=\s*this_chid\s*;/);
    assert.match(slice, /\bconst\s+stillActive\s*=\s*[\s\S]*?;\s*if\s*\(!stillActive\)\s*\{\s*return;\s*\}/s);
});

test('chat bootstrap: missing payload can only create and save first message explicitly', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    const getChatStart = source.indexOf('export async function getChat');
    assert.ok(getChatStart >= 0);
    const getChatSlice = source.slice(getChatStart, getChatStart + 4200);

    assert.match(getChatSlice, /allowNewChat\s*=\s*false/);
    assert.match(getChatSlice, /allowNotFound:\s*allowNewChat/);
    assert.match(getChatSlice, /allow_not_found:\s*allowNewChat/);
    const catchStart = getChatSlice.indexOf('} catch (error) {');
    assert.ok(catchStart >= 0);
    const catchSlice = getChatSlice.slice(catchStart, getChatSlice.indexOf('\n    }', catchStart));
    assert.doesNotMatch(catchSlice, /getChatResult\(/);

    const getChatResultStart = source.indexOf('async function getChatResult');
    assert.ok(getChatResultStart >= 0);
    const getChatResultSlice = source.slice(getChatResultStart, getChatResultStart + 900);
    assert.match(getChatResultSlice, /if\s*\(\s*allowNewChat\s*&&\s*chat\.length\s*===\s*0\s*\)/);
    assert.match(source, /await\s+getChat\(\s*\{\s*allowNewChat:\s*true\s*\}\s*\)/);
});

test('windowed payload: group tail-load does not mutate window state for background reads', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/group-chats.js'), 'utf8');

    assert.match(source, /async function loadGroupChat\(\s*chatId\s*,\s*\{\s*updateWindowState\s*=\s*false,\s*allowNotFound\s*=\s*false\s*\}\s*=\s*\{\s*\}\s*\)/);
    assert.match(source, /if\s*\(\s*updateWindowState[\s\S]*?setWindowedChatState\s*\(/s);
});

test('windowed payload: windowed patch commit is guarded and merges cursor offsets', async () => {
    const script = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const groupChats = await readFile(path.join(REPO_ROOT, 'src/scripts/group-chats.js'), 'utf8');

    // Brace-balanced extraction: a guard can't be defeated by the function
    // growing past a fixed character window (which already broke once).
    const saveChatSlice = extractFunctionBody(script, 'async function saveChatUnsafe');
    const saveGroupSlice = extractFunctionBody(groupChats, 'async function saveGroupChatUnsafe');

    for (const slice of [saveChatSlice, saveGroupSlice]) {
        assert.match(slice, /\bgetWindowedChatKey\b/);
        assert.match(slice, /const\s+expectedCursorOffset\s*=\s*windowState\.cursor\.offset\s*;/);
        assert.match(
            slice,
            /mergeWindowedChatCursorOffset\(\s*activeWindowState\?\.\s*cursor\s*,\s*cursor\s*,\s*expectedCursorOffset\s*\)/,
        );
        assert.match(slice, /activeWindowState\?\.\s*cursor\?\.\s*offset\s*===\s*expectedCursorOffset/);
        // Window baseline contract: the patch call must forward the declared
        // line count, or the backend can't reject a stale cursor.
        assert.match(slice, /expectedWindowLineCount/);
    }
});
