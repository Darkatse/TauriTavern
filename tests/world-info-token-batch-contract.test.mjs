import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
    canPrefetchWorldInfoTokenCount,
    getWorldInfoTokenPrefetchBatch,
} from '../src/scripts/world-info-token-prefetch.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('World info batches only exact safe token-count prefixes', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/world-info.js'), 'utf8');

    assert.match(source, /getTokenCountAsync, getTokenPrefixCountsAsync/);
    assert.match(source, /getWorldInfoTokenPrefetchBatch\(newEntries, entryIndex\)/);
    assert.match(source, /getTokenPrefixCountsAsync\(batchBaseContent, batchSuffixes, undefined, remainingBudget\)/);
    assert.match(source, /prefetchedTokenCounts\.has\(entry\)[\s\S]*getTokenCountAsync\(newContent\)/);
});

test('World info keeps probability, macro, budget, and activation ordering around token prefetch', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/world-info.js'), 'utf8');
    const start = source.indexOf("console.debug('[WI] --- PROBABILITY CHECKS ---')");
    const end = source.indexOf('const successfulNewEntries =', start);
    const slice = source.slice(start, end);

    const probabilityIndex = slice.indexOf('const success = verifyProbability()');
    const substitutionIndex = slice.indexOf('entry.content = substituteParams(entry.content)');
    const prefetchIndex = slice.indexOf('getWorldInfoTokenPrefetchBatch(newEntries, entryIndex)');
    const budgetIndex = slice.indexOf('(textToScanTokens + newContentTokens) >= budget');
    const activationIndex = slice.indexOf('allActivatedEntries.set');

    assert.ok(probabilityIndex >= 0);
    assert.ok(substitutionIndex > probabilityIndex);
    assert.ok(prefetchIndex > substitutionIndex);
    assert.ok(budgetIndex > prefetchIndex);
    assert.ok(activationIndex > budgetIndex);
    assert.match(slice, /const newContentTokens = entry\.ignoreBudget\s*\? 0/);
    assert.match(slice, /if \(!entry\.ignoreBudget && \(textToScanTokens \+ newContentTokens\) >= budget\)/);
});

test('World info token prefetch rejects behavior-sensitive entries', () => {
    assert.equal(canPrefetchWorldInfoTokenCount({ content: '', ignoreBudget: false, useProbability: false }), true);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: 'plain', ignoreBudget: true, useProbability: false }), false);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: 'plain', ignoreBudget: false, useProbability: true, probability: 50 }), false);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: 'plain', ignoreBudget: false, useProbability: true, probability: 100 }), true);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: '{{user}}', ignoreBudget: false, useProbability: false }), false);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: '<USER>', ignoreBudget: false, useProbability: false }), false);
});

test('World info token prefetch keeps only the contiguous safe prefix in activation order', () => {
    const entries = [
        { uid: 1, content: 'first', ignoreBudget: false, useProbability: false },
        { uid: 2, content: '', ignoreBudget: false, useProbability: false },
        { uid: 3, content: '{{dynamic}}', ignoreBudget: false, useProbability: false },
        { uid: 4, content: 'later', ignoreBudget: false, useProbability: false },
    ];

    const batch = getWorldInfoTokenPrefetchBatch(entries, 0);
    assert.deepEqual(batch.entries, entries.slice(0, 2));
    assert.deepEqual(batch.suffixes, ['first\n', '\n']);
    assert.deepEqual(entries.map(entry => entry.uid), [1, 2, 3, 4]);

    const laterBatch = getWorldInfoTokenPrefetchBatch(entries, 3);
    assert.deepEqual(laterBatch.entries, [entries[3]]);
    assert.deepEqual(laterBatch.suffixes, ['later\n']);
});

test('World info token prefetch keeps the native request bounded to 64 entries', () => {
    const entries = Array.from({ length: 70 }, (_, index) => ({
        uid: index,
        content: `entry-${index}`,
        ignoreBudget: false,
        useProbability: false,
    }));

    const batch = getWorldInfoTokenPrefetchBatch(entries, 0);
    assert.equal(batch.entries.length, 64);
    assert.equal(batch.suffixes.length, 64);
    assert.equal(batch.entries.at(-1), entries[63]);
});

test('Batch token counts preserve individual OpenAI wrapper semantics', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/tokenizers.js'), 'utf8');

    assert.match(source, /export async function getTokenCountsAsync/);
    assert.match(source, /countOpenAIMessageTokensBatchAsync\(messages\)/);
    assert.match(source, /getOpenAITextTokenCount\(count\)/);
    assert.match(source, /Promise\.all\(strings\.map\(text => getTokenCountAsync\(text, padding\)\)\)/);
});

test('Prefix token counts use compact native payload with an exact fallback', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/tokenizers.js'), 'utf8');

    assert.match(source, /export async function getTokenPrefixCountsAsync/);
    assert.match(source, /count-prefix-batch/);
    assert.match(source, /const requestBody = JSON\.stringify\(\{ base, suffixes, stop_at: stopAt \}\)/);
    assert.match(source, /countTokenPrefixesSingleFlight\(requestKey/);
    assert.match(source, /data: requestBody/);
    assert.match(source, /cacheState\.cache\[cacheKeys\[index\]\]/);
    assert.match(source, /using exact batch fallback/);
    assert.match(source, /return getTokenCountsAsync\(prefixes, padding\)/);
});
