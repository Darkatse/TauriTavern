import assert from 'node:assert/strict';
import test from 'node:test';
import { createBrowserRuntime } from './runtime.mjs';

const { window, getModule, load, startHost } = createBrowserRuntime();

try {
    await startHost();
    await load('script.js');
    const script = getModule('script.js').namespace;
    const openai = getModule('scripts/openai.js').namespace;
    const { macros, initRegisterMacros } = getModule('scripts/macros/macro-system.js').namespace;
    const { extension_settings } = getModule('scripts/extensions.js').namespace;
    const { power_user } = getModule('scripts/power-user.js').namespace;
    power_user.experimental_macro_engine = true;
    extension_settings.variables = { global: {} };
    script.chat_metadata.variables = { phase: 'live', count: '0' };
    initRegisterMacros();
    const ajax = window.jQuery.ajax;
    window.jQuery.ajax = options => options.url?.startsWith('/api/tokenizers/openai/count-batch')
        ? Promise.resolve({ token_counts: JSON.parse(options.data).map(() => 1) })
        : ajax(options);

    await test('independent assembly shares variables across prompt, stops, prefill and parameters', async () => {
        const settings = openai.normalizeChatCompletionSettingsForPromptAssembly({
            chat_completion_source: 'claude', claude_model: 'claude-3-5-sonnet-latest',
            openai_max_context: 1024, openai_max_tokens: 64, new_chat_prompt: '',
            assistant_prefill: '{{setvar::phase::prefill}}Prefill {{getvar::count}}',
        });
        settings.prompts.find(prompt => prompt.identifier === 'main').content =
            '{{setvar::phase::prompt}}Main {{greeting::1}} / {{outlet::probe}}';
        settings.prompt_order = [{ character_id: 100001, order: ['main', 'agentSystemPrompt', 'chatHistory']
            .map(identifier => ({ identifier, enabled: true })) }];
        Object.assign(openai.getAdditionalParametersForSource(settings), {
            include_body: 'phase: {{getvar::phase}}\ncount: {{getvar::count}}',
            exclude_body: '["{{getvar::phase}}"]',
            include_headers: 'X-Model: {{model}}\nX-Budget: {{maxPrompt}}',
        });
        const savedSettings = JSON.stringify(settings);
        const source = macros.captureContext();
        source.character.alternateGreetings = ['Alternate {{getvar::phase}}'];
        source.extensionPrompts = { customWIOutlet_probe: { value: 'Captured outlet' } };
        source.settings.custom_stopping_strings = '["{{getvar::phase}}/{{incvar::count}}"]';
        source.settings.custom_stopping_strings_macro = true;
        source.state.ephemeralStoppingStrings = ['literal {{char}}'];

        for (let attempt = 0; attempt < 2; attempt++) {
            const { messages, chatCompletionPayload: payload } = await openai.assembleOpenAIChatCompletionPrompt({
                settings, model: 'claude-3-5-sonnet-latest', macroContext: source,
                promptInputs: { messages: [], messageExamples: [], extensionPrompts: source.extensionPrompts },
                agentSystemPrompt: 'Complete the task.',
            });
            assert.ok(messages.some(message => message.content.includes('Main Alternate prompt / Captured outlet')), JSON.stringify(messages));
            assert.deepEqual(Array.from(payload.stop), ['prompt/1', 'literal {{char}}']);
            assert.equal(payload.assistant_prefill, 'Prefill 1');
            assert.equal(payload.custom_include_body, 'phase: prefill\ncount: 1');
            assert.equal(payload.custom_exclude_body, '["prefill"]');
            assert.equal(payload.custom_include_headers, 'X-Model: claude-3-5-sonnet-latest\nX-Budget: 960');
        }
        assert.equal(JSON.stringify(settings), savedSettings);
        assert.deepEqual(source.variables.local, { phase: 'live', count: '0' });
        assert.equal(script.chat_metadata.variables.phase, 'live');
        assert.equal(script.chat_metadata.variables.count, '0');
    });

    await test('independent assembly keeps group and example priority after live settings change', async () => {
        const originalPinExamples = power_user.pin_examples;
        const originalAjax = window.jQuery.ajax;
        try {
            power_user.pin_examples = true;
            const source = macros.captureContext();
            source.settings.isGroup = true;
            power_user.pin_examples = false;
            window.jQuery.ajax = options => options.url?.startsWith('/api/tokenizers/openai/count-batch')
                ? Promise.resolve({ token_counts: JSON.parse(options.data).map(item =>
                    /(?:example|history)-priority/.test(JSON.stringify(item)) ? 60 : 1) })
                : originalAjax(options);
            const settings = openai.normalizeChatCompletionSettingsForPromptAssembly({
                chat_completion_source: 'custom', custom_model: 'test-model',
                openai_max_context: 100, openai_max_tokens: 16,
                new_chat_prompt: 'Live solo header', new_group_chat_prompt: 'Captured group header',
            });
            settings.prompt_order = [{ character_id: 100001, order: ['main', 'agentSystemPrompt', 'dialogueExamples', 'chatHistory']
                .map(identifier => ({ identifier, enabled: true })) }];
            const { messages } = await openai.assembleOpenAIChatCompletionPrompt({
                settings, macroContext: source,
                promptInputs: {
                    messages: [{ role: 'user', content: 'history-priority' }],
                    messageExamples: [[{ content: 'example-priority', name: 'example_user' }]],
                    extensionPrompts: {},
                },
                agentSystemPrompt: 'Complete the task.',
            });
            assert.ok(messages.some(message => message.content === 'Captured group header'), JSON.stringify(messages));
            assert.ok(messages.some(message => message.content === 'example-priority'), JSON.stringify(messages));
            assert.equal(messages.some(message => message.content === 'history-priority'), false);
        } finally {
            power_user.pin_examples = originalPinExamples;
            window.jQuery.ajax = originalAjax;
        }
    });

    await test('empty explicit parameter templates do not execute character write macros', () => {
        const settings = openai.normalizeChatCompletionSettingsForPromptAssembly({
            chat_completion_source: 'custom', custom_model: 'test-model',
        });
        for (const engine of ['legacy', 'new']) {
            const context = macros.captureContext();
            context.engine = engine;
            context.character.persona = '{{incvar::emptyReads}}';
            context.variables.local.emptyReads = 0;
            const request = {};
            openai.applyAdditionalParametersToRequest(request, settings, { macroContext: context });
            assert.equal(context.variables.local.emptyReads, 0);
            assert.equal(macros.evaluateWithContext('{{persona}}', context), '1');
        }
    });

    await test('Custom Caption uses its own source bucket and actual model', async () => {
        openai.oai_settings.chat_completion_source = 'claude';
        openai.oai_settings.claude_model = 'chat-model';
        openai.oai_settings.custom_url = 'https://caption.example/v1';
        Object.assign(openai.getAdditionalParametersForSource(openai.oai_settings, 'custom'), {
            include_body: 'model_seen: {{model}}', exclude_body: '[]', include_headers: 'X-Phase: {{getvar::phase}}',
        });
        Object.assign(openai.getAdditionalParametersForSource(openai.oai_settings, 'claude'), {
            include_headers: 'X-Wrong-Bucket: true',
        });
        extension_settings.caption = {
            multimodal_api: 'custom', multimodal_model: 'custom_custom', custom_model: 'caption-model',
        };
        window.__TAURI_RUNNING__ = true;
        let request;
        window.fetch = async (url, options) => {
            assert.equal(url, '/api/openai/caption-image');
            request = JSON.parse(options.body);
            return { ok: true, json: async () => ({ caption: 'A caption.' }) };
        };
        const { getMultimodalCaption } = (await load('scripts/extensions/shared.js')).namespace;
        await getMultimodalCaption('data:image/png;base64,AA==', 'Describe it.');
        assert.equal(request.custom_include_body, 'model_seen: caption-model');
        assert.equal(request.custom_include_headers, 'X-Phase: live');
    });
} finally {
    await window.happyDOM.close();
}
