import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const indexHtml = await readFile(new URL('../src/index.html', import.meta.url), 'utf8');

test('Claude Opus 5 is selectable on supported transports', () => {
    const direct = indexHtml.match(/<select id="model_claude_select">([\s\S]*?)<\/select>/)?.[1];
    const vertex = indexHtml.match(/<optgroup label="Claude on Vertex AI">([\s\S]*?)<\/optgroup>/)?.[1];
    const bedrock = indexHtml.match(/<select id="model_aws_bedrock_select">([\s\S]*?)<\/select>/)?.[1];

    assert.match(direct, /<option value="claude-opus-5">claude-opus-5<\/option>/);
    assert.match(vertex, /<option value="claude-opus-5" data-mode="full">claude-opus-5<\/option>/);
    for (const model of [
        'us.anthropic.claude-opus-5',
        'global.anthropic.claude-opus-5',
        'anthropic.claude-opus-5',
    ]) {
        assert.ok(bedrock.includes(`<option value="${model}">${model}</option>`), model);
    }
});
