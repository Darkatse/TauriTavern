import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('Z.AI GLM 5.2 is a static model choice with 1M context', async () => {
    const [openaiSource, indexHtml] = await Promise.all([
        readFile(new URL('../src/scripts/openai.js', import.meta.url), 'utf8'),
        readFile(new URL('../src/index.html', import.meta.url), 'utf8'),
    ]);

    assert.match(indexHtml, /<option value="glm-5\.2">glm-5\.2<\/option>/);
    assert.match(openaiSource, /'glm-5\.2':\s*max_1mil/);
});
