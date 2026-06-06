import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

test('transport: normalizeChatFileName strips only upstream lowercase .jsonl suffix', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/transport.js'));
    const { normalizeChatFileName } = mod;

    assert.equal(normalizeChatFileName('  hello.jsonl'), '  hello');
    assert.equal(normalizeChatFileName('world.JSONL'), 'world.JSONL');
    assert.equal(normalizeChatFileName('world.JSONL.jsonl'), 'world.JSONL');
    assert.equal(normalizeChatFileName('already-normalized'), 'already-normalized');
    assert.equal(normalizeChatFileName(''), '');
    assert.equal(normalizeChatFileName(null), '');
});

test('transport: resolveCharacterDirectoryId treats avatarUrl as an exact avatar filename identity', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/transport.js'));
    const { resolveCharacterDirectoryId } = mod;

    assert.equal(resolveCharacterDirectoryId('Alice', 'Alice#1.png'), 'Alice#1');
    assert.equal(resolveCharacterDirectoryId('Alice', 'Alice%2FB.png'), 'Alice%2FB');
    assert.equal(resolveCharacterDirectoryId('Alice', ' Alice.png'), ' Alice');
});

test('transport: resolveCharacterDirectoryId rejects URL-like avatar identities', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/transport.js'));
    const { resolveCharacterDirectoryId } = mod;

    for (const avatarUrl of [
        'User Avatars/abc123.png',
        'thumbnail?file=foo.png',
        'thumbnail?file=my%20avatar.png',
        'Alice.png#hash',
        'Alice',
    ]) {
        assert.throws(
            () => resolveCharacterDirectoryId('Alice', avatarUrl),
            /Bad request: invalid avatar_url/,
            avatarUrl,
        );
    }
});

test('transport: resolveCharacterDirectoryId falls back to character name when avatar is missing', async () => {
    const mod = await importFresh(path.join(REPO_ROOT, 'src/scripts/tauri/chat/transport.js'));
    const { resolveCharacterDirectoryId } = mod;

    assert.equal(resolveCharacterDirectoryId('  Alice  ', null), 'Alice');
    assert.equal(resolveCharacterDirectoryId('  Alice  ', ''), 'Alice');
});
