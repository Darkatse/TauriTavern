import { expect, test } from '@rstest/core';
import { createFrontend } from '../helpers/frontend.mjs';

const { load, startHost } = await createFrontend();
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

await startHost();
await load('script.js');
const { normalizeChatCompletionSettingsForPromptAssembly } = (await load('scripts/openai.js'));
const { buildPromptAssemblySnapshot } = (await load('tauri/main/api/agent-prompt-assembly.js'));
const { createAgentSessionsApi } = (await load('tauri/main/api/agent-sessions.js'));
const { updateFrozenMacroContext } = (await load('scripts/tauritavern/agent/frozen-run-input-snapshot.js'));
const { MacroEnvBuilder } = (await load('scripts/macros/engine/MacroEnvBuilder.js'));
const { initRegisterMacros } = (await load('scripts/macros/macro-system.js'));
const { extension_settings } = (await load('scripts/extensions.js'));
const { power_user } = (await load('scripts/power-user.js'));
power_user.experimental_macro_engine = true;
initRegisterMacros();
const { setParamOmitted } = (await load('scripts/tauri/generation-params/omission.js'));
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
    const context = MacroEnvBuilder.createSessionContext({ messageCount: messages.flat().length, lastMessage: messages.flat().at(-1) }, {
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

test('Session PromptManager preserves canonical history and depth injections', async () => {
    const assembled = await assemble(history);
    const { promptSnapshot: snapshot } = assembled;
    const recordedAssistant = snapshot.messages.find(item => item.parts.some(part => part.type === 'toolCall'));
    expect(JSON.parse(JSON.stringify(recordedAssistant))).toEqual(assistant);
    expect(JSON.parse(JSON.stringify(snapshot.messages.find(item => item.role === 'tool')))).toEqual(result);

    const longerInput = await assemble([[message('user', 'Older history.')], ...history]);
    expect(longerInput.frozenRunInputSnapshot.promptInputs).toEqual(assembled.frozenRunInputSnapshot.promptInputs);

    const depthPrompt = { identifier: 'session-depth', name: 'Session depth', role: 'system', content: 'Depth instruction.', injection_position: 1, injection_depth: 1 };
    preset.prompts.push(depthPrompt);
    preset.prompt_order[0].order.push({ identifier: depthPrompt.identifier, enabled: true });
    const injected = (await assemble(history)).promptSnapshot.messages;
    const injectedAt = injected.findIndex(item => item.parts.some(part => part.text === depthPrompt.content));
    const currentAt = injected.findIndex(item => item.parts.some(part => part.text === 'Continue.'));
    expect(injectedAt).toBe(currentAt - 1);
    preset.prompts.pop();
    preset.prompt_order[0].order.pop();

    // An explicit effort overrides preset omission for this assembly only.
    preset.reasoning_effort = 'low';
    expect((await assemble(history)).promptSnapshot.generationParameters.reasoning_effort).toBe('low');
    setParamOmitted(preset, 'reasoning_effort', true);
    expect((await assemble(history, 'high')).promptSnapshot.generationParameters.reasoning_effort).toBe('high');
    expect((await assemble(history)).promptSnapshot.generationParameters.reasoning_effort).toBeUndefined();
});

test('Session send snapshots explicit/empty local and user global before await; macro writes stay in assembly', async () => {
    const script = (await load('script.js'));
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
                            macroMessages: {
                                messageCount: 1, lastMessage: message('user', dto.input.text),
                                lastUserMessage: message('user', dto.input.text), lastAssistantMessage: null,
                            },
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
        expect(started.promptSnapshot.messages.some(item => item.parts.some(part =>
            part.text === `Local=${expectedLocal};Global=captured-global;Char=;Last=Own Session input.`))).toBeTruthy();
        expect(started.promptAssemblyRequest.frozenRunInputSnapshot.variables.local).toEqual(originalVariables?.local ?? {});
        expect(started.promptAssemblyRequest.frozenRunInputSnapshot.variables.global).toEqual({ globalValue: 'captured-global' });
        expect(variables).toEqual(originalVariables);
        expect(extension_settings.variables.global.globalValue).toBe('later-live-global');
        expect(script.chat_metadata.variables.sessionValue).toBe('foreground-chat');
    }
});
