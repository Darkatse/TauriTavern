import { expect, onTestFinished, test } from '@rstest/core';
import { createFrontend, importFrontend } from '../helpers/frontend.mjs';

const { formatGenerationTimer, updateMessageGenerationInfo } = await importFrontend('scripts/message-generation-info.js');
const { getPromptCacheUsage } = await importFrontend('scripts/util/prompt-cache-usage.js');

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
    expect(element.querySelector('.mes_generation_rate').textContent).toBe('32.0 token/s');
    expect(timing.timerTitle).toContain('32.000 t/s');
    expect(element.querySelector('.mes_generation_ttft').textContent).toBe('TTFT 0.8s');
    expect(element.querySelector('.mes_generation_cache').textContent).toBe('Cache hit 70%');

    const swipe = { extra: { prompt_cache: { input_tokens: 100, cached_tokens: 0 } } };
    updateMessageGenerationInfo(element, swipe);
    expect(element.querySelector('.mes_generation_rate').textContent).toBe('');
    expect(element.querySelector('.mes_generation_ttft').textContent).toBe('');
    expect(element.querySelector('.mes_generation_cache').textContent).toBe('Cache hit 0%');

    updateMessageGenerationInfo(element, { extra: {} });
    expect(element.textContent).toBe('');
    expect(element.querySelector('.mes_generation_cache').title).toBe('');
    expect(formatGenerationTimer(message.gen_started, message.gen_started, 320).tokenRate).toBeNull();
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
        expect(getPromptCacheUsage(usage)).toEqual(expected);
    }
    expect(getPromptCacheUsage({ prompt_tokens: 1000 })).toBeNull();
    // #304: the provider temporarily reports cached tokens with a zero input count.
    const snapshots = [
        { prompt_tokens: 33665, prompt_tokens_details: { cached_tokens: 0 } },
        { prompt_tokens: 0, prompt_tokens_details: { cached_tokens: 64 } },
        { prompt_tokens: 33639, prompt_tokens_details: { cached_tokens: 512 } },
    ];
    expect(snapshots.map(getPromptCacheUsage)).toEqual([
        { input_tokens: 33665, cached_tokens: 0 },
        null,
        { input_tokens: 33639, cached_tokens: 512 },
    ]);
});

test('smooth streaming preserves Gemini usage-only terminal events and excludes secondary candidates', async () => {
    const chunks = [
        { candidates: [], usageMetadata: { promptTokenCount: 1000, cachedContentTokenCount: 700 } },
        { candidates: [{ content: { parts: [{ text: '' }] }, finishReason: 'STOP' }], usageMetadata: { promptTokenCount: 1000, cachedContentTokenCount: 700 } },
    ];
    const { load, startHost, close } = await createFrontend();
    onTestFinished(close);
    await startHost();
    const { SmoothEventSourceStream } = await load('scripts/sse-stream.js');
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
    expect(output).toEqual(data);
});
