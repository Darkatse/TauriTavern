import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const tokenizerSourceUrl = new URL('../src/scripts/tokenizers.js', import.meta.url);
const tokenCounterSourceUrl = new URL('../src/scripts/extensions/token-counter/index.js', import.meta.url);

function extractBetween(source, startMarker, endMarker) {
    const start = source.indexOf(startMarker);
    assert.notEqual(start, -1, `Missing marker: ${startMarker}`);
    const end = source.indexOf(endMarker, start + startMarker.length);
    assert.notEqual(end, -1, `Missing marker: ${endMarker}`);
    return source.slice(start, end);
}

test('Tokenizer fallback estimates UTF-8 bytes rather than UTF-16 code units', async () => {
    const source = await readFile(tokenizerSourceUrl, 'utf8');

    assert.match(source, /export const BYTES_PER_TOKEN = 3\.35;/);
    assert.match(source, /const textEncoder = new TextEncoder\(\);/);
    assert.match(source, /textEncoder\.encode\(str\)\.length \/ BYTES_PER_TOKEN/);
    assert.doesNotMatch(source, /str\.length \/ CHARACTERS_PER_TOKEN_RATIO/);
});

test('Token Counter awaits token ids and ignores stale async results', async () => {
    const [tokenizerSource, counterSource] = await Promise.all([
        readFile(tokenizerSourceUrl, 'utf8'),
        readFile(tokenCounterSourceUrl, 'utf8'),
    ]);

    assert.match(tokenizerSource, /export async function getTextTokensAsync\(/);
    assert.match(tokenizerSource, /const data = await jQuery\.ajax\(/);
    assert.match(counterSource, /await getTextTokensAsync\(tokenizerType, text\)/);
    assert.match(counterSource, /revision !== inputRevision/);
    assert.doesNotMatch(counterSource, /\bgetTextTokens\(/);
});

test('Async token count failures settle through an async Best Match fallback', async () => {
    const source = await readFile(tokenizerSourceUrl, 'utf8');

    assert.match(source, /error: function \(\) \{\s*tokenCount = settleTokenCount\(null, str, resolve\);/);
    assert.match(source, /void apiFailureTokenCountAsync\(str\)\.then\(resolve, error =>/);
    assert.match(source, /resolve\(guesstimate\(str\)\);/);
    assert.match(source, /async function apiFailureTokenCountAsync\(str\)/);
    assert.match(source, /return getTokenCountAsync\(str\);/);
});

test('OpenAI batch failure estimates misses without retrying the same backend per message', async () => {
    const source = await readFile(tokenizerSourceUrl, 'utf8');
    const syncBatch = extractBetween(
        source,
        'export function countTokensOpenAI(messages, full = false) {',
        '/**\n * Returns the token count for a message using the OpenAI tokenizer.',
    );
    const asyncBatch = extractBetween(
        source,
        'async function countOpenAIMessageTokensBatchAsync(messages, settings = null) {',
        '/**\n * Count tokens using the server API.',
    );

    for (const batchFn of [syncBatch, asyncBatch]) {
        assert.equal(batchFn.match(/jQuery\.ajax/g)?.length, 1);
        assert.doesNotMatch(batchFn, /legacyTokenizerEndpoint|\/count\?model=/);
        assert.match(batchFn, /guesstimateOpenAiMessageTokenCount\(cacheMisses\[i\]\)/);
    }
});
