import { expect, test } from '@rstest/core';
import { createFrontend } from '../helpers/frontend.mjs';

const { window, load, startHost } = await createFrontend();
const invoke = window.__TAURI__.core.invoke;
const handed = {};
window.__TAURI__.core.invoke = async (command, args, options) => {
    if (command === 'generate_chat_completion') {
        handed.sent = args.dto;
        throw { Cancelled: 'The test only reads the request.' };
    }
    if (command === 'preview_chat_completion_request') {
        handed.previewed = args.dto;
        return { source: 'custom', model: args.dto.model, endpoint: '/chat/completions', requestRaw: '{}', view: { request: {} } };
    }
    if (command === 'list_legacy_mcp_tools') {
        return {
            tools: [{
                toolId: 'atlas/lookup', nativeName: 'lookup', serverDisplayName: 'Atlas',
                description: 'Looks a place up.', inputSchema: { type: 'object', properties: {} },
            }],
            diagnostics: [{ code: 'mcp.catalog_not_cached', message: 'MCP server `Almanac` has no cached tool catalog.' }],
        };
    }
    return invoke(command, args, options);
};

await startHost();
const script = (await load('script.js'));
const openai = (await load('scripts/openai.js'));
const { power_user } = (await load('scripts/power-user.js'));
const { ToolManager } = (await load('scripts/tool-calling.js'));
const { initRegisterMacros } = (await load('scripts/macros/macro-system.js'));
power_user.experimental_macro_engine = true;
initRegisterMacros();
script.changeMainAPI('openai');
script.setOnlineStatus('Valid');
ToolManager.registerFunctionTool({ name: 'roll_dice', description: 'Rolls dice.', parameters: { type: 'object', properties: {} }, action: () => 'done' });

const ajax = window.jQuery.ajax;
window.jQuery.ajax = options => options.url?.startsWith('/api/tokenizers/openai/count-batch')
    ? Promise.resolve({ token_counts: JSON.parse(options.data).map(item => String(item.content).length) })
    : ajax(options);

const entries = [
    { identifier: 'main', name: 'Main Prompt', system_prompt: true, role: 'system', content: 'Narrate the scene.' },
    { identifier: 'style', name: 'Style', system_prompt: false, role: 'system', content: 'Keep it short.' },
    {
        identifier: 'readStateTool', name: 'Read state tool', system_prompt: false, role: 'tool_definition',
        content: 'Reads the current state.', fields: { name: 'read_state', parameters: { type: 'object', properties: {} } },
    },
    {
        identifier: 'reminder', name: 'Reminder', system_prompt: false, role: 'system', content: 'Stay in character.',
        injection_position: 1, injection_depth: 1, injection_order: 100,
    },
    {
        identifier: 'postscript', name: 'Postscript', system_prompt: false, role: 'user', content: 'Answer briefly.',
        injection_position: 2, attach_role: 'user', attach_index: -1, attach_side: 'end',
    },
];

Object.assign(openai.oai_settings, openai.normalizeChatCompletionSettingsForPromptAssembly({
    chat_completion_source: 'custom', custom_model: 'capture-model',
    openai_max_context: 4096, openai_max_tokens: 32, new_chat_prompt: '',
    squash_system_messages: true, function_calling: true, stream_openai: false,
}));
openai.oai_settings.prompts = openai.oai_settings.prompts.filter(prompt => !entries.some(entry => entry.identifier === prompt.identifier));
openai.oai_settings.prompts.push(...structuredClone(entries));
openai.oai_settings.prompt_order = [{
    character_id: 100001,
    order: ['main', 'style', 'readStateTool', 'chatHistory', 'reminder', 'postscript'].map(identifier => ({ identifier, enabled: true })),
}];
script.setExtensionPrompt('scene_note', 'It is raining.', script.extension_prompt_types.IN_CHAT, 1, false, script.extension_prompt_roles.SYSTEM);
script.setExtensionPrompt('lore-note', 'The sea is near.', script.extension_prompt_types.BEFORE_PROMPT, 0);

script.characters.push({
    name: 'Keeper', avatar: 'Keeper.png', chat: 'keeper-chat', shallow: false,
    description: '', personality: '', scenario: '', mes_example: '', first_mes: '', data: { extensions: {} },
});
script.setCharacterId(script.characters.length - 1);
script.setCharacterName('Keeper');
script.chat.push(
    { name: 'User', is_user: true, is_system: false, mes: 'Who keeps the light, {{char}}?', extra: {} },
    { name: 'Keeper', is_user: false, is_system: true, mes: 'A hidden note.', extra: {} },
    { name: 'Keeper', is_user: false, is_system: false, mes: 'I do.', extra: {} },
    { name: 'User', is_user: true, is_system: false, mes: 'Since when?', extra: {} },
);

const preview = () => window.__TAURITAVERN__.api.dev.requestInspector.preview();

test('a preview hands the backend what the next send hands it, and leaves the chat as it is', async () => {
    await preview();
    expect(script.chat[0].mes).toBe('Who keeps the light, {{char}}?');

    await expect(script.Generate('normal')).rejects.toThrow();

    expect(handed.previewed).toStrictEqual(handed.sent);
    expect(handed.sent.messages.slice(0, 2)).toEqual([
        { role: 'system', content: 'The sea is near.\nNarrate the scene.\nKeep it short.' },
        { role: 'user', content: 'Who keeps the light, Keeper?' },
    ]);
    expect(handed.sent.tools.map(tool => tool.function.name)).toEqual(['roll_dice', 'mcp__Atlas__lookup', 'read_state']);
});

test('a preview says what each message and tool came from', async () => {
    const { promptSources, toolSources, promptNames, chatHistory, mcpDiagnostics } = await preview();

    expect(promptSources.map(({ role, pieces }) => ({ role, pieces: pieces.map(({ tokens, ...piece }) => piece) }))).toEqual([
        { role: 'system', pieces: [{ type: 'extensionPrompt', identifier: 'lore-note', text: 'The sea is near.' }] },
        { role: 'system', pieces: [{ type: 'prompt', identifier: 'main', text: 'Narrate the scene.' }] },
        { role: 'system', pieces: [{ type: 'prompt', identifier: 'style', text: 'Keep it short.' }] },
        { role: 'user', pieces: [{ type: 'chat', messageId: 0, text: 'Who keeps the light, Keeper?' }] },
        { role: 'assistant', pieces: [{ type: 'chat', messageId: 2, text: 'I do.' }] },
        {
            role: 'system',
            pieces: [
                { type: 'prompt', identifier: 'reminder', depth: 1, text: 'Stay in character.' },
                { type: 'extensionPrompt', identifier: 'scene_note', depth: 1, text: 'It is raining.' },
            ],
        },
        {
            role: 'user',
            pieces: [{
                type: 'chat', messageId: 3, text: 'Since when?\n\nAnswer briefly.',
                attached: [{ identifier: 'postscript', text: 'Answer briefly.' }],
            }],
        },
    ]);
    for (const { tokens, pieces } of promptSources) {
        expect(pieces.reduce((sum, piece) => sum + piece.tokens, 0)).toBe(tokens);
        expect(pieces.every(piece => piece.tokens > 0)).toBe(true);
    }

    expect(toolSources).toEqual([
        { name: 'roll_dice', type: 'function' },
        { name: 'mcp__Atlas__lookup', type: 'mcp', server: 'Atlas' },
        { name: 'read_state', type: 'prompt', identifier: 'readStateTool' },
    ]);
    expect(promptNames).toEqual({
        main: 'Main Prompt', style: 'Style', reminder: 'Reminder', postscript: 'Postscript', readStateTool: 'Read state tool',
    });
    expect(chatHistory).toEqual({ included: 3, candidates: 3 });
    expect(mcpDiagnostics).toEqual([{ code: 'mcp.catalog_not_cached', message: 'MCP server `Almanac` has no cached tool catalog.' }]);
});

test('Prompt Manager holds each prompt after a dry run and the merged messages after a send', async () => {
    const held = () => openai.promptManager.messages.flatten()
        .filter(message => ['lore_note', 'main', 'style'].includes(message.identifier))
        .map(message => message.content);

    await script.Generate('normal', {}, true);
    expect(held()).toEqual(['The sea is near.', 'Narrate the scene.', 'Keep it short.']);

    await expect(script.Generate('normal')).rejects.toThrow();
    expect(held()).toEqual(['The sea is near.\nNarrate the scene.\nKeep it short.']);
});

test('a preview of a request that goes without part of the chat says why', async () => {
    const maxContext = openai.oai_settings.openai_max_context;
    openai.oai_settings.openai_max_context = 40;
    try {
        expect((await preview()).assemblyError).toMatch(/mandatory prompts/);
    } finally {
        openai.oai_settings.openai_max_context = maxContext;
    }
});

test('previews asked for before one starts to assemble are one preview', async () => {
    const { previewRequest } = (await load('scripts/tauritavern/request-preview.js'));
    let asked = 0;
    const ask = async () => {
        asked += 1;
        return {};
    };

    const first = previewRequest(ask);
    expect(previewRequest(ask)).toBe(first);
    await first;
    expect(asked).toBe(1);
});

test('a preview asks nothing about the reverse proxy, and the send confirms it', async () => {
    const { Popup } = (await load('scripts/popup.js'));
    const proxied = { chat_completion_source: 'openai', openai_model: 'gpt-4o', reverse_proxy: 'https://proxy.invalid/v1' };
    const settings = Object.fromEntries(Object.keys(proxied).map(key => [key, openai.oai_settings[key]]));
    const confirm = Popup.show.confirm;
    let asked = 0;
    Popup.show.confirm = async () => {
        asked += 1;
        return false;
    };
    Object.assign(openai.oai_settings, proxied);
    try {
        await preview();
        expect(asked).toBe(0);
        expect(handed.previewed.reverse_proxy).toBe(proxied.reverse_proxy);
        expect(script.online_status).toBe('Valid');

        await expect(script.Generate('normal')).rejects.toThrow();
        expect(asked).toBe(1);
    } finally {
        Object.assign(openai.oai_settings, settings);
        Popup.show.confirm = confirm;
        script.setOnlineStatus('Valid');
    }
});
