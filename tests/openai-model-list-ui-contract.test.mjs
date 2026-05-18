import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function readProjectFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('provider save-model button keeps stable i18n key while showing Chinese copy', async () => {
    const indexHtml = await readProjectFile('src/index.html');
    const saveButtons = [...indexHtml.matchAll(/<div class="menu_button menu_button_icon save_model_to_list"[^>]*>/g)].map(match => match[0]);

    assert.equal(saveButtons.length, 9);
    assert.equal(saveButtons.some(button => /data-model-source="custom"/.test(button)), false);
    assert.equal(saveButtons.every(button => /data-i18n="Save to Model List"/.test(button)), true);
    assert.match(indexHtml, /data-i18n="Save to Model List">保存到模型列表<\/div>/);
});

test('custom model list entries are appended inside select optgroups when present', async () => {
    const openaiSource = await readProjectFile('src/scripts/openai.js');

    assert.match(openaiSource, /const \$optionParent = tagName === 'select'\s*\? \(\$select\.children\('optgroup'\)\.last\(\)\.length \? \$select\.children\('optgroup'\)\.last\(\) : \$select\)/);
    assert.match(openaiSource, /\$optionParent\.append\(new Option\(modelId, modelId\)\)/);
    assert.match(openaiSource, /const \$optionParent = \$select\.children\('optgroup'\)\.last\(\)\.length \? \$select\.children\('optgroup'\)\.last\(\) : \$select/);
    assert.match(openaiSource, /\$optionParent\.append\(\$\('<option>', \{ value: id, text: id, 'data-dynamic': '1' \}\)\)/);
});
