import test from 'node:test';
import assert from 'node:assert/strict';
import { normalizeChatFileName, resolveCharacterDirectoryId } from '../src/scripts/tauri/chat/transport.js';

test('transport: normalizeChatFileName strips only upstream lowercase .jsonl suffix', () => {
    assert.equal(normalizeChatFileName('  hello.jsonl'), '  hello');
    assert.equal(normalizeChatFileName('world.JSONL'), 'world.JSONL');
    assert.equal(normalizeChatFileName('world.JSONL.jsonl'), 'world.JSONL');
    assert.equal(normalizeChatFileName('already-normalized'), 'already-normalized');
    assert.equal(normalizeChatFileName(''), '');
    assert.equal(normalizeChatFileName(null), '');
});

test('transport: resolveCharacterDirectoryId treats avatarUrl as an exact avatar filename identity', () => {
    assert.equal(resolveCharacterDirectoryId('Alice', 'Alice#1.png'), 'Alice#1');
    assert.equal(resolveCharacterDirectoryId('Alice', 'Alice%2FB.png'), 'Alice%2FB');
    assert.equal(resolveCharacterDirectoryId('Alice', ' Alice.png'), ' Alice');
});

test('transport: resolveCharacterDirectoryId rejects URL-like avatar identities', () => {
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

test('transport: resolveCharacterDirectoryId falls back to character name when avatar is missing', () => {
    assert.equal(resolveCharacterDirectoryId('  Alice  ', null), 'Alice');
    assert.equal(resolveCharacterDirectoryId('  Alice  ', ''), 'Alice');
});
