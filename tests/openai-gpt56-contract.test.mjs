import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const REASONING_I18N_KEY = 'Reasoning effort is ordered High, XHigh, Maximum. Provider adapters map these project levels to the nearest supported upstream value. Auto does not send an effort level.';

function readProjectFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('GPT-5.6 exposes the official family aliases with a 1M prompt budget', async () => {
    const [indexHtml, openaiSource] = await Promise.all([
        readProjectFile('src/index.html'),
        readProjectFile('src/scripts/openai.js'),
    ]);
    const group = indexHtml.match(/<optgroup label="GPT-5\.6">([\s\S]*?)<\/optgroup>/)?.[1];
    assert.ok(group, 'GPT-5.6 model group must exist');

    for (const model of ['gpt-5.6', 'gpt-5.6-sol', 'gpt-5.6-terra', 'gpt-5.6-luna']) {
        assert.ok(group.includes(`<option value="${model}">${model}</option>`));
    }
    assert.doesNotMatch(group, /gpt-5\.6-[a-z]+-\d{4}-\d{2}-\d{2}/);
    assert.match(openaiSource, /OPENAI_GPT56_MODEL_PATTERN\s*=\s*\/\^gpt-5/);
    assert.match(openaiSource, /getMaxContextOpenAI\(value\)[\s\S]*?OPENAI_GPT56_MODEL_PATTERN\.test\(value\)/);
});

test('reasoning controls keep Maximum above XHigh and preserve GPT-5.6 extremes', async () => {
    const [indexHtml, openaiSource, zhCn, zhTw] = await Promise.all([
        readProjectFile('src/index.html'),
        readProjectFile('src/scripts/openai.js'),
        readProjectFile('src/locales/zh-cn.json').then(JSON.parse),
        readProjectFile('src/locales/zh-tw.json').then(JSON.parse),
    ]);

    const select = indexHtml.match(/<select id="openai_reasoning_effort">([\s\S]*?)<\/select>/)?.[1];
    assert.ok(select, 'reasoning effort selector must exist');
    assert.ok(select.indexOf('value="high"') < select.indexOf('value="xhigh"'));
    assert.ok(select.indexOf('value="xhigh"') < select.indexOf('value="max"'));

    assert.match(openaiSource, /function normalizeOpenAiReasoningEffort\(effort, model\)[\s\S]*?case reasoning_effort_types\.min:\s*return 'none';/);
    assert.match(openaiSource, /case reasoning_effort_types\.max:[\s\S]*?supportsOpenAiMaxReasoningEffort\(model\)[\s\S]*?return reasoning_effort_types\.max;/);
    assert.ok(indexHtml.includes(`data-i18n="${REASONING_I18N_KEY}"`));
    assert.match(zhCn[REASONING_I18N_KEY], /高、超高、最大/);
    assert.match(zhTw[REASONING_I18N_KEY], /高、超高、最大/);
});
