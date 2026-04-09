import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('Token cache is partitioned per chat and avoids whole-load on init', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/tokenizers.js'), 'utf8');

    assert.match(source, /return `tokenCache:\$\{chatId\}`;/);
    assert.match(source, /objectStore\.removeItem\(['"]tokenCache['"]\)/);
    assert.match(source, /scheduleLegacyTokenCacheCleanup\(\);/);

    assert.doesNotMatch(source, /objectStore\.getItem\(['"]tokenCache['"]\)/);
    assert.doesNotMatch(source, /objectStore\.setItem\(['"]tokenCache['"]/);

    assert.match(source, /\blet tokenCacheState\s*=\s*\{/);
    assert.match(source, /eventSource\.on\(event_types\.CHAT_CHANGED/);
});
