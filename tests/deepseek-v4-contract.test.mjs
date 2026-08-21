import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('DeepSeek v4 models expose Vision Exp without changing the Flash default', async () => {
    const [openaiSource, indexHtml] = await Promise.all([
        readFile(new URL('../src/scripts/openai.js', import.meta.url), 'utf8'),
        readFile(new URL('../src/index.html', import.meta.url), 'utf8'),
    ]);

    assert.match(openaiSource, /deepseek_model:\s*'deepseek-v4-flash'/);
    assert.match(openaiSource, /case chat_completion_sources\.DEEPSEEK:\s*return settings\.deepseek_model === 'deepseek-v4-flash-vision-exp';/);
    assert.match(indexHtml, /<option value="deepseek-v4-flash">deepseek-v4-flash<\/option>/);
    assert.match(indexHtml, /<option value="deepseek-v4-flash-vision-exp">deepseek-v4-flash-vision-exp<\/option>/);
    assert.match(indexHtml, /<option value="deepseek-v4-pro">deepseek-v4-pro<\/option>/);
    assert.match(indexHtml, /data-source="[^"]*deepseek[^"]*">\s*<label for="openai_media_inlining"/);
    assert.match(indexHtml, /data-source="[^"]*deepseek[^"]*">\s*<div class="flex-container oneline-dropdown">\s*<label for="openai_inline_image_quality"/);
});
