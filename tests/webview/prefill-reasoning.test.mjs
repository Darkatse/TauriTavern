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
    ? Promise.resolve({ token_counts: JSON.parse(options.data).map(() => 1) })
    : ajax(options);

async function moonshotPrefill({ prefill, bias, continued }) {
    const settings = openai.normalizeChatCompletionSettingsForPromptAssembly({
        chat_completion_source: 'moonshot', moonshot_model: 'kimi-k3',
        openai_max_context: 1024, openai_max_tokens: 64, continue_prefill: true,
    });
    const order = ['main', 'chatHistory'];
    if (prefill) {
        settings.prompts.push({ identifier: 'prefill', name: 'Prefill', role: 'assistant', content: prefill, system_prompt: false });
        order.push('prefill');
    }
    settings.prompt_order = [{ character_id: 100001, order: order.map(identifier => ({ identifier, enabled: true })) }];
    const history = [{ role: 'user', content: 'hello' }];
    if (continued) {
        history.unshift({ role: 'assistant', content: continued });
    }
    const { messages } = await openai.assembleOpenAIChatCompletionPrompt({
        settings, macroContext: macros.captureContext(), agentMode: false,
        generationType: continued ? 'continue' : 'normal',
        promptInputs: { messages: history, messageExamples: [], extensionPrompts: {}, bias },
    });
    return messages.at(-1);
}

test('Moonshot sends a hidden prefill reasoning as reasoning_content', async () => {
    expect(await moonshotPrefill({ prefill: '<think>Plan the reply' }))
        .toEqual({ role: 'assistant', content: '', reasoning_content: 'Plan the reply' });
    expect(await moonshotPrefill({ prefill: '<think>\nPlan\n</think>\n\nMara' }))
        .toEqual({ role: 'assistant', content: 'Mara', reasoning_content: 'Plan' });
    expect(await moonshotPrefill({ bias: '<think>\nPlan' }))
        .toEqual({ role: 'assistant', content: '<think>\nPlan' });
    expect(await moonshotPrefill({ continued: '<think>\nPlan' }))
        .toEqual({ role: 'assistant', content: '<think>\nPlan' });
});
