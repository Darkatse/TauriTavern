import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

test('chat API ChatRef preserves exact character identity text', async () => {
    const { normalizeChatRef } = await importFresh(
        path.join(REPO_ROOT, 'src/tauri/main/api/chat-utils.js'),
    );

    assert.deepEqual(normalizeChatRef({
        kind: 'character',
        characterId: ' Alice#1',
        fileName: ' Story.jsonl',
    }), {
        kind: 'character',
        characterId: ' Alice#1',
        fileName: ' Story',
    });
});
