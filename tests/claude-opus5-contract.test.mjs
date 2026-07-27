import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const CLAUDE_EFFORT_HINT = 'Claude options: legacy thinking maps effort to a token budget. Adaptive thinking preserves XHigh and Maximum when supported. Auto uses the model default; Claude 5 models think by default.';

const [indexHtml, openaiSource, captionSettings, zhCn, zhTw] = await Promise.all([
    readFile(new URL('../src/index.html', import.meta.url), 'utf8'),
    readFile(new URL('../src/scripts/openai.js', import.meta.url), 'utf8'),
    readFile(new URL('../src/scripts/extensions/caption/settings.html', import.meta.url), 'utf8'),
    readFile(new URL('../src/locales/zh-cn.json', import.meta.url), 'utf8').then(JSON.parse),
    readFile(new URL('../src/locales/zh-tw.json', import.meta.url), 'utf8').then(JSON.parse),
]);

test('Claude Opus 5 is exposed on every supported Claude transport', () => {
    const direct = indexHtml.match(/<select id="model_claude_select">([\s\S]*?)<\/select>/)?.[1];
    const vertex = indexHtml.match(/<optgroup label="Claude on Vertex AI">([\s\S]*?)<\/optgroup>/)?.[1];
    const bedrock = indexHtml.match(/<select id="model_aws_bedrock_select">([\s\S]*?)<\/select>/)?.[1];

    assert.match(direct, /<option value="claude-opus-5">claude-opus-5<\/option>/);
    assert.match(vertex, /<option value="claude-opus-5" data-mode="full">claude-opus-5<\/option>/);
    for (const model of [
        'us.anthropic.claude-opus-5',
        'global.anthropic.claude-opus-5',
        'anthropic.claude-opus-5',
    ]) {
        assert.ok(bedrock.includes(`<option value="${model}">${model}</option>`), model);
    }
    assert.match(captionSettings, /data-type="anthropic" value="claude-opus-5"/);
});

test('Claude Opus 5 exposes its context, vision, and full effort scale', () => {
    assert.match(openaiSource, /function isClaudeOneMillionContextModel[\s\S]*?opus-5/);
    assert.ok((openaiSource.match(/isClaudeOneMillionContextModel\(/g) ?? []).length >= 4);
    assert.match(openaiSource, /if \(usesClaudeMessagesSemantics\(settings, model\)\)[\s\S]*?return settings\.reasoning_effort/);
    assert.match(openaiSource, /const visionSupportedModels = \[[\s\S]*?'claude-opus-5'/);

    assert.ok(indexHtml.includes(`data-i18n="${CLAUDE_EFFORT_HINT}"`));
    assert.match(zhCn[CLAUDE_EFFORT_HINT], /Claude 5.*默认/);
    assert.match(zhTw[CLAUDE_EFFORT_HINT], /Claude 5.*預設/);
});
