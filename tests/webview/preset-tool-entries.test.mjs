import { expect, test } from '@rstest/core';
import { createFrontend } from '../helpers/frontend.mjs';

const { window, load, startHost } = await createFrontend();
const invoke = window.__TAURI__.core.invoke;
window.__TAURI__.core.invoke = async (command, args, options) => {
    if (command === 'build_agent_current_model_connection_snapshot') {
        return { currentModelConnection: {
            schemaVersion: 1,
            kind: 'tauritavern.currentModelConnectionSnapshot',
            settings: { chat_completion_source: args.dto.settings.chat_completion_source, model: args.dto.model },
        } };
    }
    if (command === 'apply_agent_current_model_connection_snapshot') {
        return { settings: { ...args.dto.settings, ...args.dto.currentModelConnection.settings } };
    }
    return invoke(command, args, options);
};

await startHost();
const script = (await load('script.js'));
const openai = (await load('scripts/openai.js'));
const { power_user } = (await load('scripts/power-user.js'));
const { ToolManager } = (await load('scripts/tool-calling.js'));
const { macros, initRegisterMacros } = (await load('scripts/macros/macro-system.js'));
const codemirror = (await load('scripts/tauri/codemirror-editor.js'));
const { getTauriTavernSettings } = (await load('tauri-bridge.js'));
power_user.experimental_macro_engine = true;
initRegisterMacros();
script.changeMainAPI('openai');
script.chat_metadata.variables = { quote: 'Say "hi"\nthen go', hp: '3', field: 'reasoning_content', macro: '{{getvar::hp}}' };

// The read_state call costs more than the tight context below can hold.
const ajax = window.jQuery.ajax;
window.jQuery.ajax = options => options.url?.startsWith('/api/tokenizers/openai/count-batch')
    ? Promise.resolve({ token_counts: JSON.parse(options.data).map(item => JSON.stringify(item).includes('"name":"read_state"') ? 500 : 1) })
    : ajax(options);

const readStateTool = {
    identifier: 'readStateTool', name: 'Read state tool', system_prompt: false,
    role: 'tool_definition', content: 'Reads the current state.',
    fields: { name: 'read_state', parameters: { type: 'object', properties: { target: { type: 'string' } } } },
};
const readStateCall = {
    identifier: 'readStateCall', name: 'Read state call', system_prompt: false,
    role: 'assistant', content: '',
    fields: {
        tool_calls: [{
            id: 'call_1', type: 'function',
            function: { name: 'read_state', arguments: { target: '{{getvar::quote}}' } },
            signature: 'skip_thought_signature_validator',
        }],
        '{{getvar::field}}': '',
    },
};
const readStateResult = {
    identifier: 'readStateResult', name: 'Read state result', system_prompt: false,
    role: 'tool', content: 'hp: {{getvar::hp}}', fields: { tool_call_id: 'call_1' },
};
const expectedCall = {
    role: 'assistant', content: '', reasoning_content: '',
    tool_calls: [{
        id: 'call_1', type: 'function',
        function: { name: 'read_state', arguments: JSON.stringify({ target: 'Say "hi"\nthen go' }) },
        signature: 'skip_thought_signature_validator',
    }],
};
const expectedResult = { role: 'tool', content: 'hp: 3', tool_call_id: 'call_1' };

function assemble({ entries, order, history = [{ role: 'user', content: 'Latest' }], maxContext = 4096, functionCalling = false, systemPromptOverride = '' }) {
    const settings = openai.normalizeChatCompletionSettingsForPromptAssembly({
        chat_completion_source: 'openai', openai_model: 'gpt-4o', function_calling: functionCalling,
        openai_max_context: maxContext, openai_max_tokens: 16, new_chat_prompt: '',
    });
    settings.prompts = settings.prompts.filter(prompt => !entries.some(entry => entry.identifier === prompt.identifier));
    settings.prompts.push(...structuredClone(entries));
    settings.prompt_order = [{ character_id: 100001, order: order.map(identifier => ({ identifier, enabled: true })) }];
    return openai.assembleOpenAIChatCompletionPrompt({
        settings, macroContext: macros.captureContext(), agentMode: false,
        promptInputs: { messages: structuredClone(history), messageExamples: [], extensionPrompts: {}, systemPromptOverride },
    });
}

test('relative entries send a tool call, its result and the tools the preset declares', async () => {
    const { messages, chatCompletionPayload } = await assemble({
        entries: [readStateTool, readStateCall, readStateResult],
        order: ['main', 'readStateTool', 'chatHistory', 'readStateCall', 'readStateResult'],
    });

    expect(messages.slice(-3)).toEqual([{ role: 'user', content: 'Latest' }, expectedCall, expectedResult]);
    expect(chatCompletionPayload.tools).toEqual([{
        type: 'function',
        function: { name: 'read_state', description: 'Reads the current state.', parameters: readStateTool.fields.parameters },
    }]);
    expect(chatCompletionPayload.tool_choice).toBeUndefined();
});

test('in-chat entries keep their own messages and leave the history together', async () => {
    const atDepth = entry => ({ ...entry, injection_position: 1, injection_depth: 1, injection_order: 100 });
    const reminder = { identifier: 'reminder', name: 'Reminder', system_prompt: false, role: 'system', content: 'Stay in character.' };
    const entries = [atDepth(readStateCall), atDepth(readStateResult), atDepth(reminder)];
    const order = ['main', 'chatHistory', 'readStateCall', 'readStateResult', 'reminder'];
    const history = [
        { role: 'user', content: 'Latest' },
        { role: 'assistant', content: 'Earlier reply' },
        { role: 'user', content: 'Oldest' },
    ];

    const roomy = await assemble({ entries, order, history });
    expect(roomy.messages.slice(-6)).toEqual([
        { role: 'user', content: 'Oldest' },
        { role: 'assistant', content: 'Earlier reply' },
        expectedCall,
        expectedResult,
        { role: 'system', content: 'Stay in character.' },
        { role: 'user', content: 'Latest' },
    ]);

    const tight = await assemble({ entries, order, history, maxContext: 400 });
    expect(tight.messages.slice(-2)).toEqual([
        { role: 'system', content: 'Stay in character.' },
        { role: 'user', content: 'Latest' },
    ]);
    expect(tight.messages.some(message => message.role === 'tool' || message.tool_calls)).toBe(false);
});

test('an entry the request cannot express stops assembly and names the entry', async () => {
    const call = readStateCall.fields.tool_calls[0];
    for (const broken of [
        { role: 'tool', content: 'hp: 3' },
        { role: 'tool', content: '', fields: { tool_call_id: 'call_1', content: 'hp: 3' } },
        { role: 'assistant', content: '', fields: { tool_calls: [{ ...call, id: undefined }] } },
        { role: 'tool_definition', content: 'Reads.', fields: { parameters: {} } },
        { role: 'user', content: 'Hi', fields: ['not', 'an', 'object'] },
        { role: 'user', content: 'Hi', fields: { reasoning_content: 'a', '{{getvar::field}}': 'b' } },
        { role: 'assistant', content: '', injection_position: 2, fields: { tool_calls: [call] } },
    ]) {
        const entry = { identifier: 'broken', name: 'Broken entry', system_prompt: false, ...broken };
        await expect(assemble({ entries: [entry], order: ['main', 'chatHistory', 'broken'] }), JSON.stringify(broken))
            .rejects.toThrow('"Broken entry"');
    }
});

test('a character card override replaces the text of the entry and leaves its fields as they were', async () => {
    const main = {
        identifier: 'main', name: 'Main Prompt', system_prompt: true, role: 'system',
        content: 'Main: {{getvar::hp}}', fields: { note: '{{getvar::macro}}', quoted: '{{original}}' },
    };
    const order = ['main', 'chatHistory'];
    const [plain] = (await assemble({ entries: [main], order })).messages;
    const [overridden] = (await assemble({ entries: [main], order, systemPromptOverride: 'Card. {{original}}' })).messages;

    expect(plain).toEqual({ role: 'system', content: 'Main: 3', note: '{{getvar::hp}}', quoted: '{{original}}' });
    expect(overridden).toEqual({ ...plain, content: 'Card. Main: 3' });
});

test('the editor saves the entries it opens unchanged and checks fields as the assembly does', () => {
    const promptManager = openai.promptManager;
    const form = id => window.document.getElementById(`completion_prompt_manager_popup_entry_form_${id}`);
    const sent = prompt => ({ role: prompt.role, content: prompt.content, fields: prompt.fields });
    const prompts = promptManager.serviceSettings.prompts;
    prompts.push(...structuredClone([readStateTool, readStateCall]));
    try {
        for (const entry of [readStateTool, readStateCall, readStateResult]) {
            const prompt = structuredClone(entry);
            promptManager.loadPromptIntoEditForm(prompt);
            promptManager.updatePromptWithPromptEditForm(prompt);
            expect(sent(prompt)).toEqual(sent(entry));
        }

        const answer = { identifier: 'answer', name: 'Answer', role: 'system', content: 'hp: 3' };
        promptManager.loadPromptIntoEditForm(answer);
        form('role').value = 'tool';
        form('role').dispatchEvent(new window.Event('change'));
        expect(form('fields_error').textContent).toBe('Prompt "Answer": a tool response needs a tool_call_id.');
        expect(form('save').getAttribute('aria-disabled')).toBe('true');

        form('tool_call_id').value = 'call_1';
        form('tool_call_id').dispatchEvent(new window.Event('change'));
        expect(form('fields_error').textContent).toBe('');
        promptManager.updatePromptWithPromptEditForm(answer);
        expect(sent(answer)).toEqual({ role: 'tool', content: 'hp: 3', fields: { tool_call_id: 'call_1' } });
    } finally {
        promptManager.clearEditForm();
        prompts.splice(-2);
    }
});

async function withEditors(run) {
    codemirror.initializeCodeMirrorEditor({ codemirror_editor_enabled: true });
    try {
        await run();
    } finally {
        openai.promptManager.clearEditForm();
        codemirror.initializeCodeMirrorEditor(await getTauriTavernSettings());
    }
}

/** The form mounts its own editors; wait for that one rather than mount another. */
async function formEditor(field) {
    let editor;
    while (!(editor = codemirror.getMountedCodeMirrorEditor(field))) await new Promise(resolve => setTimeout(resolve, 0));
    return editor;
}

const editorRoot = editor => editor.wrapper.shadowRoot ?? editor.wrapper;
const press = (editor, label) => editorRoot(editor).querySelector(`button[aria-label="${label}"]`).click();

test('the fields toolbar rewrites JSON as it will be stored, as one undo step', () => withEditors(async () => {
    const field = window.document.getElementById('completion_prompt_manager_popup_entry_form_fields');
    openai.promptManager.loadPromptIntoEditForm({ identifier: 'answer', name: 'Answer', role: 'tool', content: 'hp: 3', fields: { tool_call_id: 'call_1' } });
    const editor = await formEditor(field);
    const edit = text => {
        field.value = text;
        editor.reset();
    };
    const text = () => {
        editor.flush();
        return field.value;
    };

    edit('{"tool_call_id":"call_1"}');
    press(editor, 'Format JSON');
    expect(text()).toBe('{\n  "tool_call_id": "call_1"\n}');
    press(editor, 'Undo');
    expect(text()).toBe('{"tool_call_id":"call_1"}');

    edit('{"tool_call_id":');
    press(editor, 'Format JSON');
    expect(text()).toBe('{"tool_call_id":');
}));

test('changing the kind of an entry keeps its text edits undoable and names the editor after the new label', () => withEditors(async () => {
    const field = window.document.getElementById('completion_prompt_manager_popup_entry_form_prompt');
    const role = window.document.getElementById('completion_prompt_manager_popup_entry_form_role');
    openai.promptManager.loadPromptIntoEditForm({ identifier: 'note', name: 'Note', role: 'system', content: 'Before' });
    const type = (editor, label, text) => {
        const input = editorRoot(editor).querySelector(`input[aria-label="${label}"]`);
        input.value = text;
        input.dispatchEvent(new window.Event('input'));
    };
    const loaded = await formEditor(field);
    press(loaded, 'Find and replace');
    type(loaded, 'Find', 'Before');
    type(loaded, 'Replace', 'After');
    press(loaded, 'Replace all');

    role.value = 'tool_call';
    role.dispatchEvent(new window.Event('change'));
    const editor = await formEditor(field);
    expect(editorRoot(editor).querySelector('[role="textbox"]').getAttribute('aria-label')).toBe('Text before the call (optional)');
    press(editor, 'Undo');
    editor.flush();
    expect(field.value).toBe('Before');
}));

test('a preset definition replaces the registered tool of the same name', async () => {
    const action = () => 'done';
    ToolManager.registerFunctionTool({ name: 'read_state', description: 'Registered.', parameters: { type: 'object', properties: {} }, action });
    ToolManager.registerFunctionTool({ name: 'roll_dice', description: 'Rolls dice.', parameters: { type: 'object', properties: {} }, action });
    try {
        const { chatCompletionPayload } = await assemble({
            entries: [readStateTool], order: ['main', 'readStateTool', 'chatHistory'], functionCalling: true,
        });
        expect(chatCompletionPayload.tools.map(tool => tool.function.name)).toEqual(['read_state', 'roll_dice']);
        expect(chatCompletionPayload.tools[0].function.description).toBe('Reads the current state.');
        expect(chatCompletionPayload.tool_choice).toBe('auto');
    } finally {
        ToolManager.unregisterFunctionTool('read_state');
        ToolManager.unregisterFunctionTool('roll_dice');
    }
});

test('Agent snapshots carry the preset tool entries from the live prompt and from independent assembly', async () => {
    const { buildAgentPromptSnapshotSeed, materializeCurrentPromptSnapshot } = (await load('tauri/main/api/agent-prompt-snapshot.js'));
    const { buildPromptAssemblySnapshot } = (await load('tauri/main/api/agent-prompt-assembly.js'));
    Object.assign(openai.oai_settings, openai.normalizeChatCompletionSettingsForPromptAssembly({
        chat_completion_source: 'custom', custom_model: 'capture-model',
        openai_max_context: 4096, openai_max_tokens: 32, new_chat_prompt: '',
    }));
    openai.oai_settings.prompts.push(...structuredClone([readStateTool, readStateCall, readStateResult]));
    openai.oai_settings.prompt_order = [{ character_id: 100001, order:
        ['main', 'agentSystemPrompt', 'readStateTool', 'chatHistory', 'readStateCall', 'readStateResult']
            .map(identifier => ({ identifier, enabled: true })) }];
    script.characters.push({
        name: 'Capture', avatar: 'Capture.png', chat: 'capture-chat', shallow: false,
        description: '', personality: '', scenario: '', mes_example: '', first_mes: '', data: { extensions: {} },
    });
    script.setCharacterId(script.characters.length - 1);
    script.setCharacterName('Capture');
    script.chat.push({ name: 'User', is_user: true, is_system: false, mes: 'Latest', extra: {} });
    const agentContextPolicy = { initialChatHistoryMessages: -1, includeActivatedWorldInfo: false };

    const seed = await buildAgentPromptSnapshotSeed({ agentContextPolicy, agentSystemPrompt: 'Inspect the state.' });
    const { promptSnapshot: live } = await materializeCurrentPromptSnapshot(seed);
    const { promptSnapshot: independent } = await buildPromptAssemblySnapshot({
        frozenRunInputSnapshot: seed.frozenRunInputSnapshot,
        settings: seed.frozenRunInputSnapshot.currentPreset.settings,
        agentContextPolicy,
        agentSystemPrompt: 'Inspect the state.',
    });

    for (const snapshot of [live, independent]) {
        expect(snapshot.generationParameters.tools.map(tool => tool.function.name)).toEqual(['read_state']);
        expect(snapshot.generationParameters).not.toHaveProperty('tool_choice');
        expect(snapshot.messages.slice(-2)).toEqual([expectedCall, expectedResult]);
    }
});
