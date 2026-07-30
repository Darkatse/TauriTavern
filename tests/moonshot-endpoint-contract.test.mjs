import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const [openaiSource, indexHtml, aiRoutesSource] = await Promise.all([
    readFile(new URL('../src/scripts/openai.js', import.meta.url), 'utf8'),
    readFile(new URL('../src/index.html', import.meta.url), 'utf8'),
    readFile(new URL('../src/tauri/main/routes/ai-routes.js', import.meta.url), 'utf8'),
]);

test('Moonshot endpoint selection reaches settings, generation, and status', () => {
    assert.match(openaiSource, /export const MOONSHOT_ENDPOINT\s*=\s*\{\s*GLOBAL: 'global',\s*CN: 'cn'/s);
    assert.match(openaiSource, /moonshot_endpoint:\s*\['#moonshot_endpoint', 'moonshot_endpoint', false, true\]/);
    assert.match(openaiSource, /moonshot_endpoint:\s*MOONSHOT_ENDPOINT\.GLOBAL/);
    assert.match(openaiSource, /generate_data\.moonshot_endpoint = settings\.moonshot_endpoint \|\| MOONSHOT_ENDPOINT\.GLOBAL/);
    assert.match(openaiSource, /data\.moonshot_endpoint = oai_settings\.moonshot_endpoint/);
    assert.match(openaiSource, /\[chat_completion_sources\.OPENROUTER, chat_completion_sources\.MOONSHOT\]\.includes\(settings\.chat_completion_source\)/);
    assert.match(indexHtml, /id="openai_reasoning_effort_block"[^>]+data-source="[^"]*moonshot/);
    assert.match(aiRoutesSource, /moonshot_endpoint:\s*String\(payload\.moonshot_endpoint \|\| ''\)/);
});

test('Moonshot form provides the current Kimi model family as static fallbacks', () => {
    const form = indexHtml.match(/<div id="moonshot_form"[\s\S]*?<div id="zai_form"/)?.[0] ?? '';
    const modelSelect = form.match(/<select id="model_moonshot_select">[\s\S]*?<\/select>/)?.[0] ?? '';
    const models = [...modelSelect.matchAll(/<option value="([^"]+)">/g)].map(match => match[1]);

    assert.match(form, /<select id="moonshot_endpoint">[\s\S]*?value="global"[\s\S]*?value="cn"/);
    assert.deepEqual(models, [
        'kimi-k2.5',
        'kimi-k2.6',
        'kimi-k2.7-code',
        'kimi-k2.7-code-highspeed',
        'kimi-k3',
    ]);
    assert.match(openaiSource, /'kimi-k2\.5': max_256k,[\s\S]*?'kimi-k2\.6': max_256k,[\s\S]*?'kimi-k2\.7-code': max_256k,[\s\S]*?'kimi-k2\.7-code-highspeed': max_256k,[\s\S]*?'kimi-k3': max_1mil/);
});
