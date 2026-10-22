import assert from 'node:assert/strict';
import { createBrowserRuntime } from './runtime.mjs';

const { window, getModule, load, startHost } = await createBrowserRuntime();
const message = (role, text) => ({ role, parts: [{ type: 'text', text }], providerMetadata: {} });
const call = { callId: 'call-1', toolId: 'builtin:workspace.read_file', arguments: { path: 'work/{{user}}.md' }, providerMetadata: { modelAlias: 'workspace_read_file', signature: 'opaque-signature' } };
const assistant = {
    role: 'assistant',
    parts: [
        { type: 'text', text: 'Reading {{user}} literally.' },
        { type: 'reasoning', text: 'checking', provider_metadata: { signature: 'reasoning-signature' } },
        { type: 'native', provider: 'claude', value: { content: [{ type: 'thinking', signature: 'native-signature' }] } },
        { type: 'media', mime_type: 'image/png', value: { type: 'image_url', image_url: { url: 'https://example.test/reference.png' } } },
        { type: 'resourceRef', uri: 'work/reference.md' },
        { type: 'toolCall', call },
    ],
    providerMetadata: { model: 'original-model' },
};
const result = { role: 'tool', parts: [{ type: 'toolResult', result: { callId: call.callId, toolId: call.toolId, content: 'Saved {{user}} content.', structured: {}, isError: false, resourceRefs: [] } }], providerMetadata: {} };
const history = [[message('user', 'Read the file.')], [assistant, result], [message('assistant', 'I read it.')], [message('user', 'Continue.')]];

try {
    await startHost();
    await load('script.js');
    const { normalizeChatCompletionSettingsForPromptAssembly } = getModule('scripts/openai.js').namespace;
    const { buildPromptAssemblySnapshot } = getModule('tauri/main/api/agent-prompt-assembly.js').namespace;
    const { createAgentSessionsApi } = getModule('tauri/main/api/agent-sessions.js').namespace;
    const { updateFrozenMacroContext } = getModule('scripts/tauritavern/agent/frozen-run-input-snapshot.js').namespace;
    const { MacroEnvBuilder } = getModule('scripts/macros/engine/MacroEnvBuilder.js').namespace;
    const { initRegisterMacros } = getModule('scripts/macros/macro-system.js').namespace;
    const { extension_settings } = getModule('scripts/extensions.js').namespace;
    const { power_user } = getModule('scripts/power-user.js').namespace;
    power_user.experimental_macro_engine = true;
    initRegisterMacros();
    const { setParamOmitted } = getModule('scripts/tauri/generation-params/omission.js').namespace;
    const preset = normalizeChatCompletionSettingsForPromptAssembly({
        chat_completion_source: 'custom', custom_model: 'test-model', openai_max_tokens: 32, new_chat_prompt: '',
    });
    preset.prompt_order = [{ character_id: 100001, order: ['main', 'agentSystemPrompt', 'chatHistory'].map(identifier => ({ identifier, enabled: true })) }];
    const sessionInput = messages => ({
        schemaVersion: 2,
        kind: 'tauritavern.agentFrozenRunInputSnapshot',
        generationType: 'normal',
        contextKind: 'session',
        worldInfoActivation: {},
        macroContext: {},
        promptInputs: { messages: [], sessionHistory: messages, messageExamples: [], worldInfoExamples: { before: [], after: [] }, extensionPrompts: {} },
    });
    const assemble = (messages, reasoningEffort) => {
        const context = MacroEnvBuilder.createSessionContext(messages.flat(), {
            sessionId: 'session-history', variables: { local: {}, global: {} },
        });
        Object.assign(context.names, { user: 'Session user', char: 'Assistant' });
        return buildPromptAssemblySnapshot({
            modelId: 'test-model',
            reasoningEffort,
            settings: preset,
            agentContextPolicy: { initialChatHistoryMessages: -1, includeActivatedWorldInfo: false },
            agentSystemPrompt: 'Inspect the workspace.',
            frozenRunInputSnapshot: updateFrozenMacroContext(sessionInput(messages), context),
        });
    };

    const assembled = await assemble(history);
    const { promptSnapshot: snapshot } = assembled;
    const recordedAssistant = snapshot.messages.find(item => item.parts.some(part => part.type === 'toolCall'));
    assert.deepEqual(JSON.parse(JSON.stringify(recordedAssistant)), assistant);
    assert.deepEqual(JSON.parse(JSON.stringify(snapshot.messages.find(item => item.role === 'tool'))), result);

    const longerInput = await assemble([[message('user', 'Older history.')], ...history]);
    assert.deepEqual(longerInput.frozenRunInputSnapshot.promptInputs, assembled.frozenRunInputSnapshot.promptInputs);

    const depthPrompt = { identifier: 'session-depth', name: 'Session depth', role: 'system', content: 'Depth instruction.', injection_position: 1, injection_depth: 1 };
    preset.prompts.push(depthPrompt);
    preset.prompt_order[0].order.push({ identifier: depthPrompt.identifier, enabled: true });
    const injected = (await assemble(history)).promptSnapshot.messages;
    const injectedAt = injected.findIndex(item => item.parts.some(part => part.text === depthPrompt.content));
    const currentAt = injected.findIndex(item => item.parts.some(part => part.text === 'Continue.'));
    assert.equal(injectedAt, currentAt - 1);
    preset.prompts.pop();
    preset.prompt_order[0].order.pop();

    // An explicit effort overrides preset omission for this assembly only.
    preset.reasoning_effort = 'low';
    assert.equal((await assemble(history)).promptSnapshot.generationParameters.reasoning_effort, 'low');
    setParamOmitted(preset, 'reasoning_effort', true);
    assert.equal((await assemble(history, 'high')).promptSnapshot.generationParameters.reasoning_effort, 'high');
    assert.equal((await assemble(history)).promptSnapshot.generationParameters.reasoning_effort, undefined);
    console.log('PASS: Session PromptManager preserves canonical history and depth injections');

    const script = getModule('script.js').namespace;
    script.chat_metadata.variables = { sessionValue: 'foreground-chat' };
    script.setCharacterName('Foreground character');
    const sessionPreset = normalizeChatCompletionSettingsForPromptAssembly({
        chat_completion_source: 'custom', custom_model: 'test-model', openai_max_tokens: 32, new_chat_prompt: '',
    });
    sessionPreset.prompt_order = structuredClone(preset.prompt_order);
    sessionPreset.prompts.find(prompt => prompt.identifier === 'main').content =
        'Local={{getvar::sessionValue}};Global={{getglobalvar::globalValue}};Char={{char}};Last={{lastMessage}}'
        + '{{setvar::sessionValue::work}}{{setglobalvar::globalValue::work}}';

    for (const variables of [{ local: { sessionValue: 'explicit-local' } }, undefined]) {
        extension_settings.variables = { global: { globalValue: 'captured-global' } };
        const originalVariables = structuredClone(variables);
        const loading = Promise.withResolvers();
        const release = Promise.withResolvers();
        let started;
        const runId = variables ? 'session-run-explicit' : 'session-run-default';
        const sessions = createAgentSessionsApi({
            promptAssembly: { buildSnapshot: buildPromptAssemblySnapshot },
            attachAssemblyBridge() {},
            async safeInvoke(command, { dto } = {}) {
                switch (command) {
                    case 'load_agent_session_profile':
                        loading.resolve();
                        await release.promise;
                        return { profile: { model: { mode: 'connectionRef', connectionRef: 'session-connection', modelId: 'test-model' } } };
                    case 'prepare_agent_session_run':
                        return {
                            expectedHistorySeq: 0,
                            sessionMessages: [message('user', dto.input.text)],
                            request: {
                                modelId: 'test-model', settings: sessionPreset,
                                agentContextPolicy: { initialChatHistoryMessages: -1, includeActivatedWorldInfo: false },
                                agentSystemPrompt: 'Inspect the workspace.',
                                frozenRunInputSnapshot: sessionInput([[message('user', dto.input.text)]]),
                            },
                        };
                    case 'start_agent_session_run':
                        started = dto;
                        return { runId, status: 'created' };
                    default: throw new Error(`Unexpected Session command: ${command}`);
                }
            },
        });
        const sending = sessions.send({ sessionId: 'session-send', text: 'Own Session input.', variables });
        await loading.promise;
        // The global snapshot must already exist before the first backend await finishes.
        extension_settings.variables.global = { globalValue: 'later-live-global' };
        release.resolve();
        await sending;
        const expectedLocal = variables ? 'explicit-local' : '';
        assert.ok(started.promptSnapshot.messages.some(item => item.parts.some(part =>
            part.text === `Local=${expectedLocal};Global=captured-global;Char=;Last=Own Session input.`)));
        assert.deepEqual(started.promptAssemblyRequest.frozenRunInputSnapshot.variables.local, originalVariables?.local ?? {});
        assert.deepEqual(started.promptAssemblyRequest.frozenRunInputSnapshot.variables.global, { globalValue: 'captured-global' });
        assert.deepEqual(variables, originalVariables);
        assert.equal(extension_settings.variables.global.globalValue, 'later-live-global');
        assert.equal(script.chat_metadata.variables.sessionValue, 'foreground-chat');
    }
    console.log('PASS: Session send snapshots explicit/empty local and user global before await; macro writes stay in assembly');
} finally {
    await window.happyDOM.close();
}
