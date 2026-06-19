import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ZAI_REASONING_EFFORT_I18N_KEY = 'Z.AI GLM-5.2 options: Auto omits the effort field, Minimum skips thinking, Low and Medium request high effort, and XHigh requests max effort.';

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

test('Z.AI GLM 5.2 exposes native reasoning effort without generic downgrades', async () => {
    const [openaiSource, indexHtml, zhCn, zhTw] = await Promise.all([
        readProjectFile('src/scripts/openai.js'),
        readProjectFile('src/index.html'),
        readProjectFile('src/locales/zh-cn.json').then(JSON.parse),
        readProjectFile('src/locales/zh-tw.json').then(JSON.parse),
    ]);

    assert.match(openaiSource, /function isZaiReasoningEffortModel\(model\)\s*{[\s\S]*=== 'glm-5\.2';[\s\S]*}/);
    assert.match(openaiSource, /if \(settings\.chat_completion_source === chat_completion_sources\.ZAI\) \{\s*return getZaiReasoningEffort\(settings, model\);\s*}/);
    assert.match(openaiSource, /function getZaiReasoningEffort\(settings, model\)\s*{[\s\S]*!settings\.show_thoughts[\s\S]*!isZaiReasoningEffortModel\(model\)[\s\S]*case reasoning_effort_types\.min:\s*return 'minimal';[\s\S]*default:\s*return settings\.reasoning_effort;/);
    const maximumResolver = openaiSource.match(/function resolveMaximumReasoningEffort\(\)\s*{([\s\S]*?)\n        }/);
    assert.ok(maximumResolver, 'generic maximum reasoning resolver must exist');
    assert.doesNotMatch(maximumResolver[1], /chat_completion_sources\.ZAI/);

    assert.match(indexHtml, /id="openai_reasoning_effort_block"[^>]*data-source="[^"]*\bzai\b[^"]*"/);
    assert.match(indexHtml, new RegExp(`data-source="zai" data-i18n="${ZAI_REASONING_EFFORT_I18N_KEY.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}"`));
    assert.equal(zhCn[ZAI_REASONING_EFFORT_I18N_KEY], 'Z.AI GLM-5.2 选项：自动不会发送推理强度字段；极低会跳过思考过程；低和中会请求 high 推理强度；超高会请求 max 推理强度。');
    assert.equal(zhTw[ZAI_REASONING_EFFORT_I18N_KEY], 'Z.AI GLM-5.2 選項：自動不會傳送推理耗費欄位；最小會略過思考過程；低和中會請求 high 推理耗費；超高會請求 max 推理耗費。');
    assert.match(openaiSource, /function updateReasoningEffortControlVisibility\(\)\s*{[\s\S]*block\.toggle\(isZaiReasoningEffortModel\(oai_settings\.zai_model\)\);[\s\S]*}/);
});
