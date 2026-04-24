import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function readProjectFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('attach existing message logic runs on raw chat history before in-chat injections', async () => {
    const openaiSource = await readProjectFile('src/scripts/openai.js');

    assert.match(openaiSource, /function applyAttachedPromptsToMessages\(attachedPrompts, messages\)/);
    assert.match(openaiSource, /applyAttachedPromptsToMessages\(attachedPrompts, messages\);[\s\S]*messages = await populationInjectionPrompts\(absolutePrompts, messages\);/);
    assert.doesNotMatch(openaiSource, /chatCompletion\.getMessages\(\)[\s\S]*attach/);
});
