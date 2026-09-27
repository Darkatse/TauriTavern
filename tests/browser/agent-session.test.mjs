import assert from 'node:assert/strict';
import { createBrowserRuntime } from './runtime.mjs';

const { window, getModule, load, startHost } = createBrowserRuntime();
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
const history = [message('user', 'Read the file.'), assistant, result, message('assistant', 'I read it.'), message('user', 'Continue.')];

try {
    await startHost();
    await load('script.js');
    // A deterministic tokenizer transport keeps this a real PromptManager test.
    const ajax = window.jQuery.ajax;
    window.jQuery.ajax = options => options.url?.startsWith('/api/tokenizers/openai/count-batch')
        ? Promise.resolve({ token_counts: JSON.parse(options.data).map(item => 4 + Math.ceil(JSON.stringify(item).length / 4)) })
        : ajax(options);
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
        schemaVersion: 1,
        kind: 'tauritavern.agentFrozenRunInputSnapshot',
        generationType: 'normal',
        contextKind: 'session',
        worldInfoActivation: {},
        macroContext: {},
        promptInputs: { messages: [], agentMessages: messages, messageExamples: [], extensionPrompts: {} },
    });
    const assemble = (messages, contextBudget = 4096, reasoningEffort) => {
        const context = MacroEnvBuilder.createSessionContext(messages, {
            sessionId: 'session-history', variables: { local: {}, global: {} },
        });
        Object.assign(context.names, { user: 'Session user', char: 'Assistant' });
        return buildPromptAssemblySnapshot({
            modelId: 'test-model',
            reasoningEffort,
            settings: { ...preset, openai_max_context: contextBudget },
            agentContextPolicy: { initialChatHistoryMessages: -1, includeActivatedWorldInfo: false },
            agentSystemPrompt: 'Inspect the workspace.',
            frozenRunInputSnapshot: updateFrozenMacroContext(sessionInput(messages), context),
        });
    };

    const { promptSnapshot: snapshot } = await assemble(history);
    const recordedAssistant = snapshot.messages.find(item => item.parts.some(part => part.type === 'toolCall'));
    assert.deepEqual(JSON.parse(JSON.stringify(recordedAssistant)), assistant);
    assert.deepEqual(JSON.parse(JSON.stringify(snapshot.messages.find(item => item.role === 'tool'))), result);

    // Budget pressure drops a whole call/result group, never one protocol half.
    const limitedInput = await assemble(history, 200);
    const limited = limitedInput.promptSnapshot.messages;
    assert.equal(limited.some(item => item.role === 'tool' || item.parts.some(part => part.type === 'toolCall')), false);
    assert.equal(limited.some(item => item.parts.some(part => part.text === 'Continue.')), true);

    // Older history outside the budget must not enlarge the input persisted for each Run.
    const longerInput = await assemble([message('user', 'Older history. '.repeat(1000)), ...history], 200);
    assert.deepEqual(longerInput.promptSnapshot, limitedInput.promptSnapshot);
    assert.deepEqual(longerInput.frozenRunInputSnapshot.promptInputs, limitedInput.frozenRunInputSnapshot.promptInputs);
    assert.equal(JSON.stringify(longerInput.frozenRunInputSnapshot).includes('Older history.'), false);

    // Same-name calls must retain their own outcomes even when replies arrive out of order.
    const unansweredCall = { ...call, callId: 'call-2', arguments: { path: 'work/pending.md' } };
    const failedCall = { ...call, callId: 'call-3', arguments: { path: 'work/missing.md' } };
    const interruptedAssistant = { ...assistant, parts: [
        ...assistant.parts,
        { type: 'toolCall', call: unansweredCall },
        { type: 'toolCall', call: failedCall },
    ] };
    const failedResult = { role: 'tool', parts: [{ type: 'toolResult', result: {
        ...result.parts[0].result, callId: failedCall.callId, content: 'File does not exist.', isError: true,
    } }], providerMetadata: {} };
    const interruptedHistory = [history[0], interruptedAssistant, failedResult, result, history.at(-1)];
    const originalHistory = JSON.stringify(interruptedHistory);
    const interrupted = (await assemble(interruptedHistory)).promptSnapshot.messages;
    assert.equal(interrupted.some(item => item.role === 'tool' || item.parts.some(part => part.type === 'toolCall')), false);
    const explanation = interrupted.find(item => item.parts.some(part => part.text?.includes(call.providerMetadata.modelAlias)))
        .parts.map(part => part.text).join('\n');
    assert.ok(explanation.includes(assistant.parts[0].text));
    const calls = [call, unansweredCall, failedCall];
    const positions = calls.map(item => explanation.indexOf(item.arguments.path));
    assert.ok(positions.every((position, index) => position >= 0 && (index === 0 || position > positions[index - 1])));
    const outcomes = positions.map((position, index) => explanation.slice(position, positions[index + 1]));
    assert.ok(explanation.includes(call.providerMetadata.modelAlias));
    for (const item of calls) {
        assert.equal(explanation.includes(item.callId), false);
    }
    assert.equal(explanation.includes(call.toolId), false);
    assert.ok(outcomes[0].includes(result.parts[0].result.content));
    assert.equal(outcomes[0].includes(failedResult.parts[0].result.content), false);
    assert.match(outcomes[1], /unknown/i);
    assert.equal(outcomes[1].includes(result.parts[0].result.content), false);
    assert.ok(outcomes[2].includes(failedResult.parts[0].result.content));
    assert.match(outcomes[2], /error/i);
    assert.equal(JSON.stringify(interruptedHistory), originalHistory);

    // An explicit effort overrides preset omission for this assembly only.
    preset.reasoning_effort = 'low';
    assert.equal((await assemble(history)).promptSnapshot.generationParameters.reasoning_effort, 'low');
    setParamOmitted(preset, 'reasoning_effort', true);
    assert.equal((await assemble(history, 4096, 'high')).promptSnapshot.generationParameters.reasoning_effort, 'high');
    assert.equal((await assemble(history)).promptSnapshot.generationParameters.reasoning_effort, undefined);
    console.log('PASS: Session PromptManager preserves canonical history, atomic tool groups and bounded Run input');

    const script = getModule('script.js').namespace;
    script.chat_metadata.variables = { sessionValue: 'foreground-chat' };
    script.setCharacterName('Foreground character');
    const sessionPreset = normalizeChatCompletionSettingsForPromptAssembly({
        chat_completion_source: 'custom', custom_model: 'test-model', openai_max_context: 4096,
        openai_max_tokens: 32, new_chat_prompt: '',
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
        const sessions = createAgentSessionsApi({
            promptAssembly: { buildSnapshot: buildPromptAssemblySnapshot },
            async safeInvoke(command, { dto } = {}) {
                switch (command) {
                    case 'load_agent_session_profile':
                        loading.resolve();
                        await release.promise;
                        return { profile: { model: { mode: 'connectionRef', connectionRef: 'session-connection', modelId: 'test-model' } } };
                    case 'prepare_agent_session_run':
                        return {
                            expectedHistorySeq: 0,
                            request: {
                                modelId: 'test-model', settings: sessionPreset,
                                agentContextPolicy: { initialChatHistoryMessages: -1, includeActivatedWorldInfo: false },
                                agentSystemPrompt: 'Inspect the workspace.',
                                frozenRunInputSnapshot: sessionInput([message('user', dto.text)]),
                            },
                        };
                    case 'start_agent_session_run':
                        started = dto;
                        return { runId: 'session-run', status: 'created' };
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
        assert.deepEqual(started.frozenRunInputSnapshot.variables.local, originalVariables?.local ?? {});
        assert.deepEqual(started.frozenRunInputSnapshot.variables.global, { globalValue: 'captured-global' });
        assert.deepEqual(variables, originalVariables);
        assert.equal(extension_settings.variables.global.globalValue, 'later-live-global');
        assert.equal(script.chat_metadata.variables.sessionValue, 'foreground-chat');
    }
    console.log('PASS: Session send snapshots explicit/empty local and user global before await; macro writes stay in assembly');
} finally {
    await window.happyDOM.close();
}
