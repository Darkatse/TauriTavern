import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

function buildMessages(count) {
    return Array.from({ length: count }, (_, index) => ({ id: index, mes: `m-${index}` }));
}

test('windowed-state: get/set/clear windowed chat state', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { clearWindowedChatState, getWindowedChatState, setWindowedChatState } = mod;

    clearWindowedChatState();
    assert.equal(getWindowedChatState(), null);

    const state = { kind: 'character', fileName: 'a', savedMessageCount: 0, dirtyFromIndex: 0 };
    setWindowedChatState(state);
    assert.equal(getWindowedChatState(), state);

    clearWindowedChatState();
    assert.equal(getWindowedChatState(), null);
});

test('windowed-state: getWindowedChatKey trims and separates kinds', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { getWindowedChatKey } = mod;

    assert.equal(getWindowedChatKey(null), '');
    assert.equal(getWindowedChatKey({ kind: 'group', id: '  grp-1  ' }), 'group:grp-1');

    assert.equal(getWindowedChatKey({
        kind: 'character',
        characterName: ' Alice ',
        avatarUrl: ' /User%20Avatars/a.png ',
        fileName: ' chat-1 ',
    }), 'character:Alice|/User%20Avatars/a.png|chat-1');
});

test('windowed-state: mergeWindowedChatCursorOffset applies header delta to active offset', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { mergeWindowedChatCursorOffset } = mod;

    assert.equal(mergeWindowedChatCursorOffset(null, null, 0), null);
    assert.deepEqual(
        mergeWindowedChatCursorOffset(null, { offset: 10, size: 1, modifiedMillis: 2 }, 10),
        { offset: 10, size: 1, modifiedMillis: 2 },
    );

    assert.deepEqual(
        mergeWindowedChatCursorOffset({ offset: 10, size: 1, modifiedMillis: 2 }, null, 10),
        { offset: 10, size: 1, modifiedMillis: 2 },
    );

    assert.deepEqual(
        mergeWindowedChatCursorOffset(
            { offset: 20, size: 1, modifiedMillis: 2 },
            { offset: 10, size: 2, modifiedMillis: 3 },
            10,
        ),
        { offset: 20, size: 2, modifiedMillis: 3 },
    );

    assert.deepEqual(
        mergeWindowedChatCursorOffset(
            { offset: 20, size: 1, modifiedMillis: 2 },
            { offset: 12, size: 2, modifiedMillis: 3 },
            10,
        ),
        { offset: 22, size: 2, modifiedMillis: 3 },
    );

    assert.deepEqual(
        mergeWindowedChatCursorOffset(
            { offset: 20, size: 1, modifiedMillis: 2 },
            { offset: 8, size: 2, modifiedMillis: 3 },
            10,
        ),
        { offset: 18, size: 2, modifiedMillis: 3 },
    );
});

test('windowed-state: mergeWindowedChatCursorOffset throws on missing or invalid base offset', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { mergeWindowedChatCursorOffset } = mod;

    assert.throws(
        () => mergeWindowedChatCursorOffset(
            { offset: 10, size: 1, modifiedMillis: 2 },
            { offset: 12, size: 2, modifiedMillis: 3 },
        ),
        /base offset/i,
    );

    assert.throws(
        () => mergeWindowedChatCursorOffset(
            { offset: 10, size: 1, modifiedMillis: 2 },
            { offset: 12, size: 2, modifiedMillis: 3 },
            NaN,
        ),
        /base offset/i,
    );

    assert.throws(
        () => mergeWindowedChatCursorOffset(
            { offset: 10, size: 1, modifiedMillis: 2 },
            { offset: 12, size: 2, modifiedMillis: 3 },
            -1,
        ),
        /base offset/i,
    );
});

test('windowed-state: buildWindowedPayloadPatch rewriteFromIndex when dirtyFromIndex < savedMessageCount', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { buildWindowedPayloadPatch } = mod;

    const messages = buildMessages(5);
    const windowState = { savedMessageCount: 5, dirtyFromIndex: 2 };
    const result = buildWindowedPayloadPatch(messages, windowState, 'chat');

    assert.equal(result.patch.kind, 'rewriteFromIndex');
    assert.equal(result.patch.startIndex, 2);
    assert.deepEqual(result.patch.lines, messages.slice(2).map((entry) => JSON.stringify(entry)));
    assert.equal(result.savedMessageCount, messages.length);
    assert.equal(result.dirtyFromIndex, messages.length);
});

test('windowed-state: buildWindowedPayloadPatch truncates when messages shorter than savedMessageCount', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { buildWindowedPayloadPatch } = mod;

    const messages = buildMessages(3);
    const windowState = { savedMessageCount: 5, dirtyFromIndex: 5 };
    const result = buildWindowedPayloadPatch(messages, windowState, 'chat');

    assert.deepEqual(result.patch, { kind: 'rewriteFromIndex', startIndex: 3, lines: [] });
    assert.equal(result.savedMessageCount, messages.length);
    assert.equal(result.dirtyFromIndex, messages.length);
});

test('windowed-state: buildWindowedPayloadPatch appends when messages longer than savedMessageCount', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { buildWindowedPayloadPatch } = mod;

    const messages = buildMessages(5);
    const windowState = { savedMessageCount: 3, dirtyFromIndex: 3 };
    const result = buildWindowedPayloadPatch(messages, windowState, 'chat');

    assert.equal(result.patch.kind, 'append');
    assert.deepEqual(result.patch.lines, messages.slice(3).map((entry) => JSON.stringify(entry)));
    assert.equal(result.savedMessageCount, messages.length);
    assert.equal(result.dirtyFromIndex, messages.length);
});

test('windowed-state: buildWindowedPayloadPatch full rewrite when unchanged but non-empty', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { buildWindowedPayloadPatch } = mod;

    const messages = buildMessages(2);
    const windowState = { savedMessageCount: 2, dirtyFromIndex: 2 };
    const result = buildWindowedPayloadPatch(messages, windowState, 'chat');

    assert.deepEqual(result.patch, {
        kind: 'rewriteFromIndex',
        startIndex: 0,
        lines: messages.map((entry) => JSON.stringify(entry)),
    });
});

test('windowed-state: buildWindowedPayloadPatch returns empty append for empty message list', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { buildWindowedPayloadPatch } = mod;

    const result = buildWindowedPayloadPatch([], { savedMessageCount: 0, dirtyFromIndex: 0 }, 'chat');
    assert.deepEqual(result.patch, { kind: 'append', lines: [] });
    assert.equal(result.savedMessageCount, 0);
    assert.equal(result.dirtyFromIndex, 0);
});

test('windowed-state: shiftWindowedMessageSaveState shifts counters without mutating original', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { shiftWindowedMessageSaveState } = mod;

    const windowState = { kind: 'character', savedMessageCount: 2, dirtyFromIndex: 1, foo: 'bar' };
    const shifted = shiftWindowedMessageSaveState(windowState, 3, 'chat');

    assert.notEqual(shifted, windowState);
    assert.equal(shifted.foo, 'bar');
    assert.equal(shifted.savedMessageCount, 5);
    assert.equal(shifted.dirtyFromIndex, 4);

    assert.equal(windowState.savedMessageCount, 2);
    assert.equal(windowState.dirtyFromIndex, 1);
});

test('windowed-state: readWindowedMessageSaveState throws on missing counters', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/windowed-state.js'));
    const { readWindowedMessageSaveState } = mod;

    assert.throws(
        () => readWindowedMessageSaveState({}, 'chat'),
        /savedMessageCount is missing/i,
    );
    assert.throws(
        () => readWindowedMessageSaveState({ savedMessageCount: 0 }, 'chat'),
        /dirtyFromIndex is missing/i,
    );
});
