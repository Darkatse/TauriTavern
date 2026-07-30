import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('GPT-5.6 aliases are selectable', async () => {
    const indexHtml = await readFile(new URL('../src/index.html', import.meta.url), 'utf8');
    const group = indexHtml.match(/<optgroup label="GPT-5\.6">([\s\S]*?)<\/optgroup>/)?.[1];
    assert.ok(group, 'GPT-5.6 model group must exist');

    for (const model of ['gpt-5.6', 'gpt-5.6-sol', 'gpt-5.6-terra', 'gpt-5.6-luna']) {
        assert.ok(group.includes(`<option value="${model}">${model}</option>`));
    }
});
