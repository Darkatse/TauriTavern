import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function readProjectFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('DeepSeek v4 is the static default and visible model choice', async () => {
    const [openaiSource, indexHtml] = await Promise.all([
        readProjectFile('src/scripts/openai.js'),
        readProjectFile('src/index.html'),
    ]);

    assert.match(openaiSource, /deepseek_model:\s*'deepseek-v4-flash'/);
    assert.match(indexHtml, /<option value="deepseek-v4-flash">deepseek-v4-flash<\/option>/);
    assert.match(indexHtml, /<option value="deepseek-v4-pro">deepseek-v4-pro<\/option>/);
});

test('DeepSeek reasoning controls match the v4 request contract', async () => {
    const [openaiSource, indexHtml] = await Promise.all([
        readProjectFile('src/scripts/openai.js'),
        readProjectFile('src/index.html'),
    ]);

    assert.match(openaiSource, /chat_completion_sources\.DEEPSEEK/);
    assert.match(openaiSource, /function resolveMaximumReasoningEffort\(\)\s*{[\s\S]*chat_completion_sources\.DEEPSEEK[\s\S]*return reasoning_effort_types\.max;[\s\S]*return reasoning_effort_types\.high;/);
    assert.match(openaiSource, /case reasoning_effort_types\.max:\s*return resolveMaximumReasoningEffort\(\);/);
    assert.match(openaiSource, /case reasoning_effort_types\.xhigh:\s*return supportsXHighReasoningEffort\(\)\s*\?\s*reasoning_effort_types\.xhigh\s*:\s*resolveMaximumReasoningEffort\(\);/);
    assert.match(indexHtml, /data-source="[^"]*\bdeepseek\b[^"]*"[\s\S]*?<select id="openai_reasoning_effort">/);
    assert.match(indexHtml, /data-source-mode="except" data-source="deepseek,zai,moonshot"/);
    assert.match(indexHtml, /DeepSeek options: Auto omits the effort field, Minimum through High request high effort, and Maximum requests max effort\./);
});

test('DeepSeek tool-call reasoning is persisted and replayed only for the same DeepSeek model turn owner', async () => {
    const [scriptSource, openaiSource, toolCallingSource] = await Promise.all([
        readProjectFile('src/script.js'),
        readProjectFile('src/scripts/openai.js'),
        readProjectFile('src/scripts/tool-calling.js'),
    ]);

    assert.match(toolCallingSource, /static async saveFunctionToolInvocations\(invocations, native = null, reasoningContent = null\)/);
    assert.match(toolCallingSource, /tool_reasoning_content:\s*reasoningContent/);
    assert.match(scriptSource, /saveFunctionToolInvocations\(invocationResult\.invocations, native, reasoningContent\)/);
    assert.match(scriptSource, /saveFunctionToolInvocations\(invocationResult\.invocations, native, toolReasoning\)/);
    assert.match(openaiSource, /const canReplayProviderTurnMetadata = isSameModel && !isOtherGroupMember/);
    assert.match(openaiSource, /const shouldReplayReasoningContent = currentApi === chat_completion_sources\.DEEPSEEK\s*&& oai_settings\.show_thoughts\s*&& canReplayProviderTurnMetadata/);
    assert.match(openaiSource, /reasoning_content:\s*message\.reasoningContent/);
    assert.match(openaiSource, /reasoning_content:\s*item\.reasoningContent/);
});
