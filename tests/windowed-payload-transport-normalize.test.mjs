import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

test('transport: normalizeChatFileName strips .jsonl and trims', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/transport.js'));
    const { normalizeChatFileName } = mod;

    assert.equal(normalizeChatFileName('  hello.jsonl  '), 'hello');
    assert.equal(normalizeChatFileName('world.JSONL'), 'world');
    assert.equal(normalizeChatFileName('already-normalized'), 'already-normalized');
    assert.equal(normalizeChatFileName(''), '');
    assert.equal(normalizeChatFileName(null), '');
});

test('transport: resolveCharacterDirectoryId prefers avatar internal id', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/transport.js'));
    const { resolveCharacterDirectoryId } = mod;

    assert.equal(resolveCharacterDirectoryId('Alice', 'User Avatars/abc123.png'), 'abc123');
    assert.equal(resolveCharacterDirectoryId('Alice', 'thumbnail?file=foo.png'), 'foo');
    assert.equal(resolveCharacterDirectoryId('Alice', 'thumbnail?file=my%20avatar.png'), 'my avatar');
});

test('transport: resolveCharacterDirectoryId falls back to character name when avatar is missing', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/transport.js'));
    const { resolveCharacterDirectoryId } = mod;

    assert.equal(resolveCharacterDirectoryId('  Alice  ', null), 'Alice');
    assert.equal(resolveCharacterDirectoryId('  Alice  ', ''), 'Alice');
});
