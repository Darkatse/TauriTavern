import assert from 'node:assert/strict';
import { register } from 'node:module';
import test, { after } from 'node:test';
import { installFakeDom } from './helpers/fake-dom.mjs';
import { getPromptCacheUsage } from '../src/scripts/util/prompt-cache-usage.js';

// Isolate frontend startup dependencies while loading the actual renderer, i18n and stream modules.
const stubs = {
    [new URL('../src/lib.js', import.meta.url).href]: "export { default as moment } from 'moment';",
    [new URL('../src/scripts/power-user.js', import.meta.url).href]: 'export const power_user = { smooth_streaming_speed: 50 };',
    [new URL('../src/scripts/utils.js', import.meta.url).href]: "export { setTimeout as delay } from 'node:timers/promises';",
};
register(`data:text/javascript,${encodeURIComponent(`
    const stubs = ${JSON.stringify(stubs)};
    export function load(url, context, nextLoad) {
        return Object.hasOwn(stubs, url)
            ? { format: 'module', source: stubs[url], shortCircuit: true }
            : nextLoad(url, context);
    }
`)}`, import.meta.url);

const dom = installFakeDom();
after(() => dom.cleanup());
const { formatGenerationTimer, updateMessageGenerationInfo } = await import('../src/scripts/message-generation-info.js');
const { SmoothEventSourceStream } = await import('../src/scripts/sse-stream.js');

test('message header uses local tokens and clears metadata when changing swipes', () => {
    const element = document.createElement('div');
    element.innerHTML = '<small class="mes_generation_ttft"></small><small class="mes_generation_rate"></small><small class="mes_generation_cache"></small>';
    const message = {
        gen_started: '2026-09-05T12:00:00Z',
        gen_finished: '2026-09-05T12:00:10Z',
        extra: { token_count: 320, time_to_first_token: 800, prompt_cache: { input_tokens: 1000, cached_tokens: 700 } },
    };
    const timing = formatGenerationTimer(message.gen_started, message.gen_finished, message.extra.token_count);
    updateMessageGenerationInfo(element, message, timing.tokenRate);
    assert.equal(element.querySelector('.mes_generation_rate').textContent, '32.0 token/s');
    assert.ok(timing.timerTitle.includes('32.000 t/s'));
    assert.equal(element.querySelector('.mes_generation_ttft').textContent, 'TTFT 0.8s');
    assert.equal(element.querySelector('.mes_generation_cache').textContent, 'Cache hit 70%');

    const swipe = { extra: { prompt_cache: { input_tokens: 100, cached_tokens: 0 } } };
    updateMessageGenerationInfo(element, swipe);
    assert.equal(element.querySelector('.mes_generation_rate').textContent, '');
    assert.equal(element.querySelector('.mes_generation_ttft').textContent, '');
    assert.equal(element.querySelector('.mes_generation_cache').textContent, 'Cache hit 0%');

    updateMessageGenerationInfo(element, { extra: {} });
    assert.equal(element.textContent, '');
    assert.equal(element.querySelector('.mes_generation_cache').title, '');
    assert.equal(formatGenerationTimer(message.gen_started, message.gen_started, 320).tokenRate, null);
});

test('cache usage distinguishes unknown from zero and includes Claude cache writes in total input', () => {
    const expected = { input_tokens: 1000, cached_tokens: 700 };
    for (const usage of [
        { prompt_tokens: 1000, prompt_tokens_details: { cached_tokens: 700 } },
        { prompt_tokens: 1000, cached_tokens: 700 },
        { prompt_tokens: 1000, prompt_cache_hit_tokens: 700 },
        { input_tokens: 100, cache_creation_input_tokens: 200, cache_read_input_tokens: 700 },
        { promptTokenCount: 1000, cachedContentTokenCount: 700 },
    ]) {
        assert.deepEqual(getPromptCacheUsage(usage), expected);
    }
    assert.equal(getPromptCacheUsage({ prompt_tokens: 1000 }), null);
    assert.deepEqual(getPromptCacheUsage({ prompt_tokens: 1000, prompt_tokens_details: { cached_tokens: 0 } }),
        { input_tokens: 1000, cached_tokens: 0 });
    assert.throws(() => getPromptCacheUsage({ prompt_tokens: 100, cached_tokens: 101 }), /Invalid prompt cache usage/);
});

test('smooth streaming preserves Gemini usage-only terminal events and excludes secondary candidates', async () => {
    const chunks = [
        { candidates: [], usageMetadata: { promptTokenCount: 1000, cachedContentTokenCount: 700 } },
        { candidates: [{ content: { parts: [{ text: '' }] }, finishReason: 'STOP' }], usageMetadata: { promptTokenCount: 1000, cachedContentTokenCount: 700 } },
    ];
    const stream = new SmoothEventSourceStream();
    const output = [];
    const consume = (async () => {
        for await (const event of stream.readable) output.push(event.data);
    })();
    const writer = stream.writable.getWriter();
    const data = [...chunks.map(chunk => JSON.stringify(chunk)), '[DONE]'];
    await writer.write(new TextEncoder().encode(`data: ${JSON.stringify({ candidates: [{ index: 1, content: { parts: [{ text: 'other candidate' }] } }] })}\n\n`));
    await writer.write(new TextEncoder().encode(data.map(chunk => `data: ${chunk}\n\n`).join('')));
    await writer.close();
    await consume;
    assert.deepEqual(output, data);
});
