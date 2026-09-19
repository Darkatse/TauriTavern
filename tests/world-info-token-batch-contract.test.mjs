import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
    canPrefetchWorldInfoTokenCount,
    getWorldInfoTokenPrefetchBatch,
} from '../src/scripts/world-info-token-prefetch.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function deterministicTokenCount(text) {
    return String(text).length;
}

function countDeterministicPrefixes(base, suffixes, stopAt) {
    let content = base;
    const counts = [];

    for (const suffix of suffixes) {
        content += suffix;
        const count = deterministicTokenCount(content);
        counts.push(count);

        if (Number.isFinite(stopAt) && count >= stopAt) {
            while (counts.length < suffixes.length) {
                counts.push(count);
            }
            break;
        }
    }

    return counts;
}

function evaluateWorldInfoBudget(inputEntries, { budget, textToScanTokens = 0, prefetch }) {
    const entries = inputEntries.map(entry => ({ ...entry }));
    const activated = [];
    const prefetchedTokenCounts = new Map();
    const batchSizes = [];
    let newContent = '';
    let tokenBudgetOverflowed = false;
    let ignoresBudget = entries.filter(entry => entry.ignoreBudget).length;

    for (let entryIndex = 0; entryIndex < entries.length; entryIndex++) {
        const entry = entries[entryIndex];
        ignoresBudget -= entry.ignoreBudget ? 1 : 0;
        if (tokenBudgetOverflowed && !entry.ignoreBudget) {
            if (ignoresBudget > 0) {
                continue;
            }
            break;
        }

        if (entry.useProbability && entry.probability !== 100 && entry.probabilityPass === false) {
            continue;
        }

        entry.content = entry.resolvedContent ?? entry.content;
        const batchBaseContent = newContent;
        newContent += `${entry.content}\n`;

        if (prefetch && canPrefetchWorldInfoTokenCount(entry) && !prefetchedTokenCounts.has(entry)) {
            const batch = getWorldInfoTokenPrefetchBatch(entries, entryIndex);
            const counts = countDeterministicPrefixes(batchBaseContent, batch.suffixes, budget - textToScanTokens);
            batch.entries.forEach((batchEntry, index) => prefetchedTokenCounts.set(batchEntry, counts[index]));
            batchSizes.push(batch.entries.length);
        }

        const newContentTokens = entry.ignoreBudget
            ? 0
            : prefetchedTokenCounts.get(entry) ?? deterministicTokenCount(newContent);
        if (!entry.ignoreBudget && textToScanTokens + newContentTokens >= budget) {
            tokenBudgetOverflowed = true;
            continue;
        }

        activated.push(entry.uid);
    }

    return { activated, newContent, tokenBudgetOverflowed, batchSizes };
}



test('World info token prefetch rejects behavior-sensitive entries', () => {
    assert.equal(canPrefetchWorldInfoTokenCount({ content: '', ignoreBudget: false, useProbability: false }), true);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: 'plain', ignoreBudget: true, useProbability: false }), false);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: 'plain', ignoreBudget: false, useProbability: true, probability: 50 }), false);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: 'plain', ignoreBudget: false, useProbability: true, probability: 100 }), true);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: '{{user}}', ignoreBudget: false, useProbability: false }), false);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: '<USER>', ignoreBudget: false, useProbability: false }), false);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: '<charIfNotGroup>', ignoreBudget: false, useProbability: false }), false);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: '{"name":"Alice"}', ignoreBudget: false, useProbability: false }), true);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: '<section>plain HTML</section>', ignoreBudget: false, useProbability: false }), true);
    assert.equal(canPrefetchWorldInfoTokenCount({ content: 'score < limit', ignoreBudget: false, useProbability: false }), true);
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

test('World info token prefetch preserves budget decisions across sensitive entry boundaries', () => {
    const fixtures = [
        {
            name: 'macro substitution, probability and ignore-budget entries',
            budget: 200,
            entries: [
                { uid: 1, content: '{{user}}', resolvedContent: 'Alice', ignoreBudget: false, useProbability: false },
                { uid: 2, content: 'plain', ignoreBudget: false, useProbability: false },
                { uid: 3, content: 'rolled-out', ignoreBudget: false, useProbability: true, probability: 50, probabilityPass: false },
                { uid: 4, content: 'guaranteed', ignoreBudget: false, useProbability: true, probability: 100 },
                { uid: 5, content: 'free', ignoreBudget: true, useProbability: false },
            ],
        },
        {
            name: 'overflow skips paid entries but still activates a later ignore-budget entry',
            budget: 10,
            textToScanTokens: 2,
            entries: [
                { uid: 1, content: 'aa', ignoreBudget: false, useProbability: false },
                { uid: 2, content: 'bbbb', ignoreBudget: false, useProbability: false },
                { uid: 3, content: 'cccccc', ignoreBudget: false, useProbability: false },
                { uid: 4, content: 'free', ignoreBudget: true, useProbability: false },
                { uid: 5, content: 'after-free', ignoreBudget: false, useProbability: false },
            ],
        },
    ];

    for (const fixture of fixtures) {
        const baseline = evaluateWorldInfoBudget(fixture.entries, { ...fixture, prefetch: false });
        const optimized = evaluateWorldInfoBudget(fixture.entries, { ...fixture, prefetch: true });

        assert.deepEqual(
            {
                activated: optimized.activated,
                newContent: optimized.newContent,
                tokenBudgetOverflowed: optimized.tokenBudgetOverflowed,
            },
            {
                activated: baseline.activated,
                newContent: baseline.newContent,
                tokenBudgetOverflowed: baseline.tokenBudgetOverflowed,
            },
            fixture.name,
        );
        assert.ok(optimized.batchSizes.length > 0, `${fixture.name} should exercise prefix batching`);
    }
});

test('World info token prefetch preserves activation order across the 64-entry batch boundary', () => {
    const entries = Array.from({ length: 65 }, (_, index) => ({
        uid: index + 1,
        content: `entry-${index + 1}`,
        ignoreBudget: false,
        useProbability: false,
    }));

    const baseline = evaluateWorldInfoBudget(entries, { budget: 10_000, prefetch: false });
    const optimized = evaluateWorldInfoBudget(entries, { budget: 10_000, prefetch: true });

    assert.deepEqual(optimized.activated, baseline.activated);
    assert.equal(optimized.newContent, baseline.newContent);
    assert.deepEqual(optimized.batchSizes, [64, 1]);
});
