import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function readProjectFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

function extractZaiContextMap(openaiSource) {
    const match = openaiSource.match(/function getZaiMaxContext[\s\S]*?const contextMap = \{([\s\S]*?)\};/);
    assert.ok(match, 'Z.AI context map must exist');
    return match[1];
}

function extractZaiModelOptions(indexHtml) {
    const match = indexHtml.match(/<select id="model_zai_select">([\s\S]*?)<\/select>/);
    assert.ok(match, 'Z.AI model select must exist');
    return match[1];
}

test('Z.AI GLM 5.2 is a static model choice with 1M context', async () => {
    const [openaiSource, indexHtml] = await Promise.all([
        readProjectFile('src/scripts/openai.js'),
        readProjectFile('src/index.html'),
    ]);

    const modelOptions = extractZaiModelOptions(indexHtml);
    assert.match(modelOptions, /<option value="glm-5\.2">glm-5\.2<\/option>/);

    const contextMap = extractZaiContextMap(openaiSource);
    assert.match(contextMap, /'glm-5\.2':\s*max_1mil/);
    assert.match(contextMap, /'glm-5\.1':\s*max_200k/);
    assert.match(contextMap, /'glm-5-turbo':\s*max_200k/);
    assert.match(contextMap, /'glm-5v-turbo':\s*max_200k/);
    assert.ok(
        contextMap.indexOf("'glm-5.2'") < contextMap.indexOf("'glm-5'"),
        'glm-5.2 must be checked before the generic glm-5 match',
    );
});
