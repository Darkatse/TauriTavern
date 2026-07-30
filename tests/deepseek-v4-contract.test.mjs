import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('DeepSeek v4 models are selectable with Flash as the default', async () => {
    const [openaiSource, indexHtml] = await Promise.all([
        readFile(new URL('../src/scripts/openai.js', import.meta.url), 'utf8'),
        readFile(new URL('../src/index.html', import.meta.url), 'utf8'),
    ]);

    assert.match(openaiSource, /deepseek_model:\s*'deepseek-v4-flash'/);
    assert.match(indexHtml, /<option value="deepseek-v4-flash">deepseek-v4-flash<\/option>/);
    assert.match(indexHtml, /<option value="deepseek-v4-pro">deepseek-v4-pro<\/option>/);
});
