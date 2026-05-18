import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function readProjectFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

const supportedProviderConstants = [
    'OPENAI',
    'OPENROUTER',
    'CUSTOM',
    'CLAUDE',
    'MAKERSUITE',
    'VERTEXAI',
    'DEEPSEEK',
    'COHERE',
    'GROQ',
    'MOONSHOT',
    'NANOGPT',
    'CHUTES',
    'SILICONFLOW',
    'ZAI',
];

test('additional parameters button is not restricted to only custom/claude/deepseek providers', async () => {
    const indexHtml = await readProjectFile('src/index.html');
    const buttonMatch = indexHtml.match(/<div[^>]+id="customize_additional_parameters"[^>]*>/);
    assert.ok(buttonMatch, 'additional parameters button should exist');

    const buttonHtml = buttonMatch[0];
    assert.doesNotMatch(
        buttonHtml,
        /data-source="custom,claude,deepseek"|data-source="custom,deepseek,claude"/,
        'button must not be limited to the original three providers',
    );
});

test('additional parameters are provider-scoped and attached to generation/status requests', async () => {
    const openaiSource = await readProjectFile('src/scripts/openai.js');

    assert.match(openaiSource, /additional_parameters_by_source/);
    assert.match(openaiSource, /function getAdditionalParametersSourceKey/);
    assert.match(openaiSource, /function applyAdditionalParametersToRequest/);
    assert.match(openaiSource, /applyAdditionalParametersToRequest\(generate_data, settings\)/);
    assert.match(openaiSource, /applyAdditionalParametersToRequest\(data, oai_settings, \{ includeBody: false \}\)/);

    for (const provider of supportedProviderConstants) {
        assert.match(
            openaiSource,
            new RegExp(`chat_completion_sources\\.${provider}`),
            `missing ${provider} source constant in additional-parameter capable code path`,
        );
    }
});
