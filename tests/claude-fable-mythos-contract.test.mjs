import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const openaiSource = await readFile(new URL('../src/scripts/openai.js', import.meta.url), 'utf8');
const indexHtml = await readFile(new URL('../src/index.html', import.meta.url), 'utf8');

test('Claude Fable 5 and Mythos 5 are first-party Claude models', () => {
    assert.match(indexHtml, /<option value="claude-fable-5">claude-fable-5<\/option>/);
    assert.match(indexHtml, /<option value="claude-mythos-5">claude-mythos-5<\/option>/);
    assert.match(openaiSource, /function isClaudeOneMillionContextModel[\s\S]*?fable-5\|mythos-5/);
    assert.match(openaiSource, /'claude-fable-5'/);
    assert.match(openaiSource, /'claude-mythos-5'/);
});

test('AWS Bedrock static fallback exposes Fable without advertising Mythos runtime support', () => {
    assert.match(indexHtml, /<option value="us\.anthropic\.claude-fable-5">/);
    assert.match(indexHtml, /<option value="global\.anthropic\.claude-fable-5">/);
    assert.match(indexHtml, /<option value="anthropic\.claude-fable-5">/);
    assert.doesNotMatch(indexHtml, /\banthropic\.claude-mythos-5\b/);

    const bedrockXHighGate = openaiSource.match(/chat_completion_sources\.AWS_BEDROCK\) \{\s*return \/(.*?)\/\.test\(model\);/s);
    assert.ok(bedrockXHighGate, 'AWS Bedrock xhigh gate should exist');
    assert.match(bedrockXHighGate[1], /fable-5/);
    assert.doesNotMatch(bedrockXHighGate[1], /mythos-5/);
});
