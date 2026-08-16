import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function extractBetween(source, startMarker, endMarker) {
    const start = source.indexOf(startMarker);
    assert.notEqual(start, -1, `Missing marker: ${startMarker}`);
    const end = source.indexOf(endMarker, start + startMarker.length);
    assert.notEqual(end, -1, `Missing marker: ${endMarker}`);
    return source.slice(start, end);
}

test('Token cache is partitioned per chat and storage failures remain non-blocking', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/tokenizers.js'), 'utf8');
    const loadFn = extractBetween(source, 'function startTokenCacheLoad(state) {', 'async function flushTokenCacheState(state) {');
    const flushFn = extractBetween(source, 'async function flushTokenCacheState(state) {', 'function getTokenCacheState(chatId) {');

    assert.match(source, /return `tokenCache:\$\{chatId\}`;/);
    assert.match(source, /objectStore\.removeItem\(['"]tokenCache['"]\)/);
    assert.match(source, /scheduleLegacyTokenCacheCleanup\(\);/);

    assert.doesNotMatch(source, /objectStore\.getItem\(['"]tokenCache['"]\)/);
    assert.doesNotMatch(source, /objectStore\.setItem\(['"]tokenCache['"]/);

    assert.match(source, /\blet tokenCacheState\s*=\s*\{/);
    assert.match(source, /eventSource\.on\(event_types\.CHAT_CHANGED/);
    assert.match(loadFn, /Ignoring invalid token cache bucket/);
    assert.match(loadFn, /\.catch\(error => console\.warn/);
    assert.doesNotMatch(loadFn, /throw new Error/);
    assert.match(flushFn, /try \{[\s\S]*await objectStore\.setItem[\s\S]*state\.dirty = false;[\s\S]*catch \(error\) \{[\s\S]*console\.warn/);
    assert.doesNotMatch(source, /queueMicrotask/);
    assert.match(source, /eventSource\.on\(event_types\.CHAT_DELETED, removeChatTokenCache\);/);
    assert.match(source, /eventSource\.on\(event_types\.GROUP_CHAT_DELETED, removeChatTokenCache\);/);
});
