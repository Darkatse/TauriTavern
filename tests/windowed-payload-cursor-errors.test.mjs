import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

test('prompt-backfill: isWindowedCursorInvalidError matches signature mismatch', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/prompt-backfill.js'));
    const { isWindowedCursorInvalidError } = mod;

    assert.equal(isWindowedCursorInvalidError(new Error('Cursor signature mismatch')), true);
    assert.equal(isWindowedCursorInvalidError('Bad Request: Cursor signature mismatch'), true);
});

test('prompt-backfill: isWindowedCursorInvalidError matches out-of-bounds and line boundary', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/prompt-backfill.js'));
    const { isWindowedCursorInvalidError } = mod;

    assert.equal(
        isWindowedCursorInvalidError('Internal error: Cursor offset is out of bounds for /tmp/chat.jsonl'),
        true,
    );
    assert.equal(
        isWindowedCursorInvalidError('Validation error: Cursor offset is not a line boundary'),
        true,
    );
});

test('prompt-backfill: isWindowedCursorInvalidError matches before-chat-payload-body error', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/prompt-backfill.js'));
    const { isWindowedCursorInvalidError } = mod;

    assert.equal(
        isWindowedCursorInvalidError('Cursor offset is before chat payload body for /tmp/chat.jsonl'),
        true,
    );
});

test('prompt-backfill: isWindowedCursorInvalidError ignores non-cursor errors', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/prompt-backfill.js'));
    const { isWindowedCursorInvalidError } = mod;

    assert.equal(isWindowedCursorInvalidError(new Error('boom')), false);
    assert.equal(isWindowedCursorInvalidError('permission denied: no'), false);
});

test('prompt-backfill: isWindowedCursorInvalidError requires known cursor failure hints', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/prompt-backfill.js'));
    const { isWindowedCursorInvalidError } = mod;

    assert.equal(isWindowedCursorInvalidError('cursor is present but message is unrelated'), false);
    assert.equal(isWindowedCursorInvalidError({ message: 'cursor: ???' }), false);
});
