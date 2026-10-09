import assert from 'node:assert/strict';
import { createBrowserRuntime } from './runtime.mjs';

const { window, getModule, load, startHost } = createBrowserRuntime();
const invoke = window.__TAURI__.core.invoke;
window.__TAURI__.core.invoke = async (command, args, options) => {
    if (command === 'build_agent_current_model_connection_snapshot') {
        return { currentModelConnection: {
            schemaVersion: 1,
            kind: 'tauritavern.currentModelConnectionSnapshot',
            settings: { chat_completion_source: args.dto.settings.chat_completion_source, model: args.dto.model },
        } };
    }
    return invoke(command, args, options);
};

try {
    await startHost();
    const script = (await load('script.js')).namespace;
    const { power_user } = getModule('scripts/power-user.js').namespace;
    const { extension_settings } = getModule('scripts/extensions.js').namespace;
    const openai = getModule('scripts/openai.js').namespace;
    const { buildAgentPromptSnapshotSeed } = getModule('tauri/main/api/agent-prompt-snapshot.js').namespace;

    const ajax = window.jQuery.ajax;
    window.jQuery.ajax = options => options.url?.startsWith('/api/tokenizers/openai/count-batch')
        ? Promise.resolve({ token_counts: JSON.parse(options.data).map(item => 4 + Math.ceil(JSON.stringify(item).length / 4)) })
        : ajax(options);
    Object.assign(openai.oai_settings, openai.normalizeChatCompletionSettingsForPromptAssembly({
        chat_completion_source: 'custom', custom_model: 'capture-model', openai_max_tokens: 32,
        openai_max_context: 4096, new_chat_prompt: '',
    }));
    script.changeMainAPI('openai');
    power_user.experimental_macro_engine = true;
    getModule('scripts/macros.js').namespace.initMacros();
    script.characters.push({
        name: 'Capture', avatar: 'Capture.png', chat: 'capture-chat', shallow: false,
        description: '{{incvar::fieldReads}}|{{getvar::phase}}', personality: '', scenario: '', mes_example: '', first_mes: '',
        data: { extensions: {} },
    });
    script.setCharacterId(0);
    script.setCharacterName('Capture');
    script.chat.push({ name: 'User', is_user: true, is_system: false, mes: 'Hello', extra: {} });
    script.chat_metadata.variables = { fieldReads: 0, phase: 'before' };
    extension_settings.variables.global = { token: 'before-global' };

    const entered = Promise.withResolvers();
    const release = Promise.withResolvers();
    script.eventSource.on(script.event_types.GENERATE_AFTER_DATA, async () => {
        entered.resolve();
        await release.promise;
        script.chat_metadata.variables.phase = 'after';
        extension_settings.variables.global.token = 'after-global';
    });
    const generation = buildAgentPromptSnapshotSeed({
        generationType: 'normal',
        agentContextPolicy: { initialChatHistoryMessages: -1, includeActivatedWorldInfo: false },
        agentSystemPrompt: 'Inspect the captured inputs.',
    });
    await entered.promise;
    release.resolve();
    const seed = await generation;
    const saved = seed.frozenRunInputSnapshot;
    assert.equal(saved.variables.local.phase, 'after');
    assert.equal(saved.variables.global.token, 'after-global');
    assert.equal(saved.variables.local.fieldReads, 1);
    assert.equal(script.chat_metadata.variables.fieldReads, 1, 'Capturing raw fields must not execute the character write macro again');
    assert.equal(saved.macroContext.character.description, '1|before');

    console.log('PASS: real Agent dry-run captures awaited extension changes without repeating character write macros');
} finally {
    await window.happyDOM.close();
}
