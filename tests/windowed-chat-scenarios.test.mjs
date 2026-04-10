import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

function cloneJson(value) {
    return JSON.parse(JSON.stringify(value));
}

function buildMessage(id, overrides = {}) {
    return {
        id,
        name: id % 2 === 0 ? 'User' : 'Assistant',
        is_user: id % 2 === 0,
        is_system: false,
        mes: `message-${id}`,
        extra: { n: id },
        ...overrides,
    };
}

function buildMessages(count, startId = 0) {
    return Array.from({ length: count }, (_, index) => buildMessage(startId + index));
}

function cloneMessages(messages) {
    return messages.map((message) => cloneJson(message));
}

function applyWindowedPatch(fileMessages, cursorOffset, patch) {
    if (!patch) {
        throw new Error('Patch is missing');
    }

    if (patch.kind === 'append') {
        const appended = (patch.lines || []).map((line) => JSON.parse(line));
        return [...fileMessages, ...appended];
    }

    if (patch.kind === 'rewriteFromIndex') {
        const startIndex = Number(patch.startIndex);
        if (!Number.isFinite(startIndex) || startIndex < 0) {
            throw new Error('Invalid rewriteFromIndex startIndex');
        }

        const absoluteStart = Number(cursorOffset) + startIndex;
        if (!Number.isFinite(absoluteStart) || absoluteStart < 0) {
            throw new Error('Invalid cursor offset');
        }

        const rewritten = (patch.lines || []).map((line) => JSON.parse(line));
        return [...fileMessages.slice(0, absoluteStart), ...rewritten];
    }

    throw new Error(`Unknown patch kind: ${patch.kind}`);
}

function assertWindowInvariant(fileMessages, windowStartIndex, chatWindow) {
    assert.ok(Array.isArray(fileMessages));
    assert.ok(Array.isArray(chatWindow));
    assert.equal(windowStartIndex >= 0, true);
    assert.deepEqual(fileMessages.slice(windowStartIndex), chatWindow);
}

test('windowed chat: long chat patch sequence preserves suffix invariant (>= 100 messages)', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { buildWindowedPayloadPatch, shiftWindowedMessageSaveState } = mod;

    let nextId = 0;
    let fileMessages = buildMessages(120, nextId);
    nextId += fileMessages.length;

    const initialWindowLines = 50;
    let windowStartIndex = fileMessages.length - initialWindowLines;
    let chatWindow = cloneMessages(fileMessages.slice(windowStartIndex));

    assert.equal(fileMessages.length, 120);
    assert.equal(chatWindow.length, initialWindowLines);

    /** @type {{ savedMessageCount: number, dirtyFromIndex: number }} */
    let windowState = { savedMessageCount: chatWindow.length, dirtyFromIndex: chatWindow.length };

    // 1) Save without changes => full rewrite of the window suffix (contract: no no-op patch).
    {
        const result = buildWindowedPayloadPatch(chatWindow, windowState, 'chat');
        assert.equal(result.patch.kind, 'rewriteFromIndex');
        assert.equal(result.patch.startIndex, 0);
        assert.equal(result.patch.lines.length, chatWindow.length);

        fileMessages = applyWindowedPatch(fileMessages, windowStartIndex, result.patch);
        windowState = { savedMessageCount: result.savedMessageCount, dirtyFromIndex: result.dirtyFromIndex };
        assertWindowInvariant(fileMessages, windowStartIndex, chatWindow);
    }

    // 2) Append new messages => append-only patch.
    {
        const appended = [buildMessage(nextId++), buildMessage(nextId++), buildMessage(nextId++)];
        chatWindow.push(...cloneMessages(appended));

        const result = buildWindowedPayloadPatch(chatWindow, windowState, 'chat');
        assert.equal(result.patch.kind, 'append');
        assert.equal(result.patch.lines.length, appended.length);

        fileMessages = applyWindowedPatch(fileMessages, windowStartIndex, result.patch);
        windowState = { savedMessageCount: result.savedMessageCount, dirtyFromIndex: result.dirtyFromIndex };
        assertWindowInvariant(fileMessages, windowStartIndex, chatWindow);
    }

    // 3) Edit an earlier in-window message => rewriteFromIndex from dirtyFromIndex.
    {
        const editedIndex = 5;
        chatWindow[editedIndex].mes = `edited-${Date.now()}`;
        windowState.dirtyFromIndex = Math.min(windowState.dirtyFromIndex, editedIndex);

        const result = buildWindowedPayloadPatch(chatWindow, windowState, 'chat');
        assert.equal(result.patch.kind, 'rewriteFromIndex');
        assert.equal(result.patch.startIndex, editedIndex);

        fileMessages = applyWindowedPatch(fileMessages, windowStartIndex, result.patch);
        windowState = { savedMessageCount: result.savedMessageCount, dirtyFromIndex: result.dirtyFromIndex };
        assert.equal(fileMessages[windowStartIndex + editedIndex].mes, chatWindow[editedIndex].mes);
        assertWindowInvariant(fileMessages, windowStartIndex, chatWindow);
    }

    // 4) Delete a middle message => rewriteFromIndex that removes it from disk.
    {
        const deletedIndex = 10;
        chatWindow.splice(deletedIndex, 1);
        windowState.dirtyFromIndex = Math.min(windowState.dirtyFromIndex, deletedIndex);

        const result = buildWindowedPayloadPatch(chatWindow, windowState, 'chat');
        assert.equal(result.patch.kind, 'rewriteFromIndex');
        assert.equal(result.patch.startIndex, deletedIndex);

        fileMessages = applyWindowedPatch(fileMessages, windowStartIndex, result.patch);
        windowState = { savedMessageCount: result.savedMessageCount, dirtyFromIndex: result.dirtyFromIndex };
        assertWindowInvariant(fileMessages, windowStartIndex, chatWindow);
    }

    // 5) Show more messages (prepend) => shift counters, then append more and save.
    {
        const prependCount = 20;
        const before = cloneMessages(fileMessages.slice(windowStartIndex - prependCount, windowStartIndex));
        assert.equal(before.length, prependCount);

        chatWindow.splice(0, 0, ...before);
        windowState = shiftWindowedMessageSaveState(windowState, before.length, 'chat');
        windowStartIndex -= before.length;

        assert.equal(windowState.savedMessageCount, chatWindow.length);
        assert.equal(windowState.dirtyFromIndex, chatWindow.length);
        assertWindowInvariant(fileMessages, windowStartIndex, chatWindow);

        chatWindow.push(cloneJson(buildMessage(nextId++)));
        const result = buildWindowedPayloadPatch(chatWindow, windowState, 'chat');
        assert.equal(result.patch.kind, 'append');

        fileMessages = applyWindowedPatch(fileMessages, windowStartIndex, result.patch);
        windowState = { savedMessageCount: result.savedMessageCount, dirtyFromIndex: result.dirtyFromIndex };
        assertWindowInvariant(fileMessages, windowStartIndex, chatWindow);
    }
});

test('windowed chat: showMore shifts dirtyFromIndex for unsaved edits (>= 100 messages)', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { buildWindowedPayloadPatch, shiftWindowedMessageSaveState } = mod;

    let fileMessages = buildMessages(120);
    const windowLines = 50;
    let windowStartIndex = fileMessages.length - windowLines;
    let chatWindow = cloneMessages(fileMessages.slice(windowStartIndex));

    const dirtyIndex = 10;
    chatWindow[dirtyIndex].mes = `dirty-${Date.now()}`;

    /** @type {{ savedMessageCount: number, dirtyFromIndex: number }} */
    let windowState = { savedMessageCount: chatWindow.length, dirtyFromIndex: dirtyIndex };

    const prependCount = 20;
    const before = cloneMessages(fileMessages.slice(windowStartIndex - prependCount, windowStartIndex));
    chatWindow.splice(0, 0, ...before);
    windowState = shiftWindowedMessageSaveState(windowState, before.length, 'chat');
    windowStartIndex -= before.length;

    assert.equal(windowState.savedMessageCount, windowLines + prependCount);
    assert.equal(windowState.dirtyFromIndex, dirtyIndex + prependCount);

    const result = buildWindowedPayloadPatch(chatWindow, windowState, 'chat');
    assert.equal(result.patch.kind, 'rewriteFromIndex');
    assert.equal(result.patch.startIndex, windowState.dirtyFromIndex);

    fileMessages = applyWindowedPatch(fileMessages, windowStartIndex, result.patch);
    assertWindowInvariant(fileMessages, windowStartIndex, chatWindow);
});

test('windowed chat: truncate-at-end patch works for long windows (>= 100 messages)', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { buildWindowedPayloadPatch } = mod;

    let fileMessages = buildMessages(140);
    const windowStartIndex = 40;
    let chatWindow = cloneMessages(fileMessages.slice(windowStartIndex));

    /** clean state */
    const windowState = { savedMessageCount: chatWindow.length, dirtyFromIndex: chatWindow.length };

    // Drop 15 messages from the end without marking earlier dirty => truncate patch.
    chatWindow = chatWindow.slice(0, chatWindow.length - 15);
    const result = buildWindowedPayloadPatch(chatWindow, windowState, 'chat');
    assert.deepEqual(result.patch, { kind: 'rewriteFromIndex', startIndex: chatWindow.length, lines: [] });

    fileMessages = applyWindowedPatch(fileMessages, windowStartIndex, result.patch);
    assertWindowInvariant(fileMessages, windowStartIndex, chatWindow);
});

