import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const openaiSource = await readFile(new URL('../src/scripts/openai.js', import.meta.url), 'utf8');
const toolCallingSource = await readFile(new URL('../src/scripts/tool-calling.js', import.meta.url), 'utf8');
const indexHtml = await readFile(new URL('../src/index.html', import.meta.url), 'utf8');

test('AWS Bedrock frontend consumes backend model metadata as the support matrix', () => {
    assert.match(openaiSource, /export function getAwsBedrockModelMetadata/);
    assert.match(openaiSource, /tauritavern\?\.bedrock/);
    assert.match(openaiSource, /model_list\.filter\(isAwsBedrockModelSupported\)/);
    assert.doesNotMatch(openaiSource, /SUPPORTED_PROVIDERS/);
    assert.doesNotMatch(openaiSource, /const isSupportedModel\s*=/);
});

test('AWS Bedrock chat completion source follows alphabetical channel ordering', () => {
    const sourceSelectMatch = indexHtml.match(/<select id="chat_completion_source">([\s\S]*?)<\/select>/);
    assert.ok(sourceSelectMatch, 'chat completion source select should exist');
    const sourceSelect = sourceSelectMatch[1];

    const aimlIndex = sourceSelect.indexOf('value="aimlapi"');
    const bedrockIndex = sourceSelect.indexOf('value="aws_bedrock"');
    const azureIndex = sourceSelect.indexOf('value="azure_openai"');

    assert.ok(aimlIndex >= 0, 'AI/ML API option should exist');
    assert.ok(bedrockIndex >= 0, 'AWS Bedrock option should exist');
    assert.ok(azureIndex >= 0, 'Azure OpenAI option should exist');
    assert.ok(aimlIndex < bedrockIndex, 'AWS Bedrock should sort after AI/ML API');
    assert.ok(bedrockIndex < azureIndex, 'AWS Bedrock should sort before Azure OpenAI');
});

test('AWS Bedrock feature gates are driven by backend capabilities', () => {
    assert.match(
        toolCallingSource,
        /case chat_completion_sources\.AWS_BEDROCK:[\s\S]*?capabilities\?\.tools === true/,
    );
    assert.match(openaiSource, /capabilities\?\.webSearch/);
    assert.match(openaiSource, /getAwsBedrockModelCapabilities\([^)]*\)\?\.images/);

    const webSearchBlocks = indexHtml.match(/data-source="[^"]*"[\s\S]{0,240}?openai_enable_web_search/g) ?? [];
    assert.ok(webSearchBlocks.length > 0, 'web search control should remain discoverable in index.html');
    assert.ok(
        webSearchBlocks.every(block => !/\baws_bedrock\b/.test(block)),
        'AWS Bedrock web search must stay disabled unless backend metadata grows a supported capability',
    );
});
