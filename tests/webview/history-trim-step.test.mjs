import { expect, test } from '@rstest/core';
import { createFrontend } from '../helpers/frontend.mjs';

const { window, load, startHost } = await createFrontend();

await startHost();
await load('script.js');
const openai = await load('scripts/openai.js');
const { macros, initRegisterMacros } = await load('scripts/macros/macro-system.js');
initRegisterMacros();
const ajax = window.jQuery.ajax;
window.jQuery.ajax = options => options.url?.startsWith('/api/tokenizers/openai/count-batch')
    ? Promise.resolve({ token_counts: JSON.parse(options.data).map(() => 3) })
    : ajax(options);

const EXAMPLE_DIALOGUES = [[{ content: 'example a' }], [{ content: 'example b' }], [{ content: 'example c' }]];

/**
 * Assembles a prompt for a chat of `chatLength` messages that outgrows the context.
 * @param {number} chatLength
 * @param {number} [historyTrimStep] - Left out, as in settings saved before the option existed.
 * @param {number} [noteDepth] - Injects a one-token note into the chat at this depth.
 * @param {number[]} [emptyNumbers] - Chat messages left empty, which are never sent.
 * @returns {Promise<{ history: number[], examples: string[] }>} Chat message numbers sent, oldest first, and example texts sent.
 */
async function assemble(chatLength, historyTrimStep, noteDepth, emptyNumbers = []) {
    const settings = openai.normalizeChatCompletionSettingsForPromptAssembly({
        chat_completion_source: 'moonshot', moonshot_model: 'kimi-k3',
        openai_max_context: 96, openai_max_tokens: 16,
        ...(historyTrimStep === undefined ? {} : { history_trim_step: historyTrimStep }),
    });
    const order = ['main', 'dialogueExamples', 'chatHistory'];
    if (noteDepth !== undefined) {
        settings.prompts.push({
            identifier: 'note', name: 'Note', system_prompt: false, role: 'system', content: 'note',
            injection_position: 1, injection_depth: noteDepth, injection_order: 100,
        });
        order.push('note');
    }
    settings.prompt_order = [{
        character_id: 100001,
        order: order.map(identifier => ({ identifier, enabled: true })),
    }];
    const chat = Array.from({ length: chatLength }, (_, number) => ({
        role: number % 2 ? 'assistant' : 'user', content: emptyNumbers.includes(number) ? '' : `message ${number}`, sourceCount: 1,
    }));
    const { messages } = await openai.assembleOpenAIChatCompletionPrompt({
        settings, macroContext: macros.captureContext(), agentMode: false, generationType: 'normal',
        // Prompt assembly receives the chat newest first.
        promptInputs: { messages: chat.reverse(), messageExamples: EXAMPLE_DIALOGUES, extensionPrompts: {} },
    });
    const texts = messages.map(message => String(message.content));
    return {
        history: texts.filter(text => text.startsWith('message ')).map(text => Number(text.slice('message '.length))),
        examples: texts.filter(text => text.startsWith('example ')),
    };
}

const CHAT_LENGTHS = Array.from({ length: 40 }, (_, offset) => 200 + offset);

test('without a step, the start of the history moves with every new message', async () => {
    const starts = [];
    for (const length of CHAT_LENGTHS) {
        const { history } = await assemble(length, 0);
        expect(history.at(-1)).toBe(length - 1);
        expect(history.length).toBeLessThan(length);
        starts.push(history[0]);
    }
    expect(new Set(starts).size).toBe(CHAT_LENGTHS.length);
});

test('with a step, the history starts on a multiple of the step and stays there until the chat grows past it', async () => {
    const step = 10;
    const starts = [];
    for (const length of CHAT_LENGTHS) {
        const natural = (await assemble(length, 0)).history;
        const { history } = await assemble(length, step);

        expect(history.at(-1)).toBe(length - 1);
        expect(history).toEqual(Array.from({ length: history.length }, (_, offset) => history[0] + offset));
        expect(history[0] % step).toBe(0);
        expect(history[0]).toBeGreaterThanOrEqual(natural[0]);
        expect(history[0] - natural[0]).toBeLessThan(step);
        starts.push(history[0]);
    }
    // 40 replies cross the step boundary about four times.
    expect(new Set(starts).size).toBeLessThanOrEqual(Math.ceil(CHAT_LENGTHS.length / step) + 1);
});

test('the space a step frees is not handed to the sections filled after the history', async () => {
    for (const length of CHAT_LENGTHS) {
        expect((await assemble(length, 10)).examples).toEqual((await assemble(length, 0)).examples);
    }
});

test('settings without a saved step trim in steps of 20', async () => {
    for (const length of CHAT_LENGTHS) {
        expect((await assemble(length)).history).toEqual((await assemble(length, 20)).history);
    }
});

test('a note injected where the budget runs out does not empty the history', async () => {
    const length = 230;
    // The budget fits this many one-token messages; a note one message shallower is the last thing that fits.
    const fits = (await assemble(length, 0)).history.length;
    for (let depth = fits - 3; depth <= fits + 1; depth += 1) {
        const natural = (await assemble(length, 0, depth)).history;
        const { history } = await assemble(length, 20, depth);
        expect(history.length).toBeGreaterThan(0);
        expect(history.at(-1)).toBe(length - 1);
        expect(history).toEqual(Array.from({ length: history.length }, (_, offset) => history[0] + offset));
        expect(history[0] % 20 === 0 || history[0] === natural[0]).toBe(true);
    }
});

test('an empty newest message does not let the step empty the history', async () => {
    // A step larger than what fits puts the boundary on the newest message, 200, which is empty and never sent.
    const length = 201;
    const { history } = await assemble(length, 100, undefined, [length - 1]);
    expect(history.length).toBeGreaterThan(0);
    expect(history.at(-1)).toBe(length - 2);
});

test('a chat that fits the context is sent whole', async () => {
    const { history } = await assemble(12, 10);
    expect(history).toEqual(Array.from({ length: 12 }, (_, number) => number));
});

test('an invalid step stops assembly even when the chat fits the context', async () => {
    for (const step of [1.5, -1, '20']) {
        await expect(assemble(12, step)).rejects.toThrow('prompt_assembly.history_trim_step_invalid');
    }
});

test('the field saves only a whole number of 0 or more', () => {
    // The test host stops before app start attaches the Chat Completion settings handlers.
    openai.initOpenAI();
    const input = window.document.getElementById('history_trim_step');
    const error = window.document.getElementById('history_trim_step_error');
    const type = value => {
        input.value = value;
        window.jQuery(input).trigger('input');
    };

    type('30');
    expect(openai.oai_settings.history_trim_step).toBe(30);
    for (const value of ['1.5', '-1', '']) {
        type(value);
        expect(openai.oai_settings.history_trim_step).toBe(30);
        expect(input.getAttribute('aria-invalid')).toBe('true');
        expect(error.textContent).not.toBe('');
    }
    type('20');
    expect(openai.oai_settings.history_trim_step).toBe(20);
    expect(input.hasAttribute('aria-invalid')).toBe(false);
    expect(error.textContent).toBe('');
});
