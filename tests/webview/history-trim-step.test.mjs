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
 * @returns {Promise<{ history: number[], examples: string[] }>} Chat message numbers sent, oldest first, and example texts sent.
 */
async function assemble(chatLength, historyTrimStep) {
    const settings = openai.normalizeChatCompletionSettingsForPromptAssembly({
        chat_completion_source: 'moonshot', moonshot_model: 'kimi-k3',
        openai_max_context: 96, openai_max_tokens: 16, history_trim_step: historyTrimStep,
    });
    settings.prompt_order = [{
        character_id: 100001,
        order: ['main', 'dialogueExamples', 'chatHistory'].map(identifier => ({ identifier, enabled: true })),
    }];
    const chat = Array.from({ length: chatLength }, (_, number) => ({
        role: number % 2 ? 'assistant' : 'user', content: `message ${number}`, sourceCount: 1,
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

test('a chat that fits the context is sent whole', async () => {
    const { history } = await assemble(12, 10);
    expect(history).toEqual(Array.from({ length: 12 }, (_, number) => number));
});
