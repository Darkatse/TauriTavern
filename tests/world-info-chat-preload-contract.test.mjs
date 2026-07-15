import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('chat changes preload world info without preparing discarded entries', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/world-info.js'), 'utf8');
    const chatChangedHandler = source.match(/eventSource\.on\(event_types\.CHAT_CHANGED,[\s\S]*?\n\s*}\);/)?.[0] ?? '';
    const preloadFunction = source.match(/async function preloadWorldInfoEntries\(\)[\s\S]*?\n}\n\nexport async function getSortedEntries/)?.[0] ?? '';
    const sortedFunction = source.match(/export async function getSortedEntries[\s\S]*?\n}\n\n\n\/\*\*/)?.[0] ?? '';

    assert.match(chatChangedHandler, /await preloadWorldInfoEntries\(\);/);
    assert.doesNotMatch(chatChangedHandler, /getSortedEntries\(/);

    assert.match(preloadFunction, /await collectWorldInfoEntries\(\);/);
    assert.doesNotMatch(preloadFunction, /prepareWorldInfoEntries|structuredClone|entries-sort/);

    assert.match(sortedFunction, /await collectWorldInfoEntries\(\)/);
    assert.match(sortedFunction, /prepareWorldInfoEntries/);
    assert.match(sortedFunction, /structuredClone\(entries\)/);
});

test('world info collection preserves prefetch and loaded-event behavior', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/world-info.js'), 'utf8');
    const collectFunction = source.match(/async function collectWorldInfoEntries[\s\S]*?\n}\n\nasync function preloadWorldInfoEntries/)?.[0] ?? '';

    assert.match(collectFunction, /prefetchWorldInfos\(worldsToPrefetch\)/);
    assert.match(collectFunction, /getGlobalLore\(\)/);
    assert.match(collectFunction, /getCharacterLore\(\)/);
    assert.match(collectFunction, /getChatLore\(\)/);
    assert.match(collectFunction, /getPersonaLore\(\)/);
    assert.match(collectFunction, /eventSource\.emit\(event_types\.WORLDINFO_ENTRIES_LOADED, \{ globalLore, characterLore, chatLore, personaLore \}\)/);
});
