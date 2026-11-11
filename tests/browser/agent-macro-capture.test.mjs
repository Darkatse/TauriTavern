import assert from 'node:assert/strict';
import { createBrowserRuntime } from './runtime.mjs';

const { window, getModule, load, startHost } = await createBrowserRuntime();
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

try {
    await startHost();
    const script = (await load('script.js')).namespace;
    const { power_user, persona_description_positions } = getModule('scripts/power-user.js').namespace;
    const { extension_settings } = getModule('scripts/extensions.js').namespace;
    const openai = getModule('scripts/openai.js').namespace;
    const { buildAgentPromptSnapshotSeed, materializeCurrentPromptSnapshot } = getModule('tauri/main/api/agent-prompt-snapshot.js').namespace;

    const ajax = window.jQuery.ajax;
    window.jQuery.ajax = options => {
        if (options.url?.startsWith('/api/tokenizers/openai/count-batch')) {
            return Promise.resolve({ token_counts: JSON.parse(options.data).map(item => 4 + Math.ceil(JSON.stringify(item).length / 4)) });
        }
        if (options.url?.startsWith('/api/tokenizers/openai/count-prefix-batch')) {
            const { base, suffixes } = JSON.parse(options.data);
            let prefix = base;
            return Promise.resolve({ token_counts: suffixes.map(suffix => Math.ceil((prefix += suffix).length / 4)) });
        }
        return ajax(options);
    };
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
    script.eventSource.once(script.event_types.GENERATE_AFTER_DATA, async () => {
        entered.resolve();
        await release.promise;
        script.chat_metadata.variables.phase = 'after';
        extension_settings.variables.global.token = 'after-global';
        openai.oai_settings.preset_settings_openai = 'Capture preset';
        openai.oai_settings.temp_openai = 0.37;
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
    assert.equal(saved.currentPreset.name, 'Capture preset');
    assert.equal(saved.currentPreset.settings.temperature, 0.37);

    console.log('PASS: real Agent dry-run captures awaited extension changes without repeating character write macros');

    const worldInfo = getModule('scripts/world-info.js').namespace;
    const { buildPromptAssemblySnapshot } = getModule('tauri/main/api/agent-prompt-assembly.js').namespace;
    const positions = worldInfo.world_info_position;
    const entries = [
        [positions.before, 'WI before'], [positions.after, 'WI after'],
        [positions.EMTop, `<START>\n${script.name1}: WI example before`],
        [positions.EMBottom, `<START>\n${script.name1}: WI example after`],
        [positions.atDepth, 'WI depth'], [positions.outlet, 'WI outlet'],
        [positions.ANTop, 'WI AN before'], [positions.ANBottom, 'WI AN after'],
    ].map(([position, content], uid) => ({
        ...structuredClone(worldInfo.newWorldInfoEntryTemplate), uid, position, content,
        constant: true, depth: 1, role: 0, outletName: 'projection',
    }));
    worldInfo.worldInfoCache.set('Projection', { entries: Object.fromEntries(entries.map(entry => [entry.uid, entry])) });
    worldInfo.selected_world_info.push('Projection');
    script.characters[0].mes_example = `<START>\n${script.name1}: Character example`;
    script.chat[0].mes = 'Recent chat';
    script.chat.unshift({ name: script.name1, is_user: true, is_system: false, mes: 'Older chat', extra: {} });
    Object.assign(script.chat_metadata, { note_position: 1, note_depth: 1, note_role: 0 });
    power_user.persona_description = 'Persona note';
    openai.oai_settings.prompts.find(prompt => prompt.identifier === 'main').content = 'Outlet: {{outlet::projection}}';
    openai.oai_settings.prompt_order = [{ character_id: 100001, order:
        ['main', 'agentSystemPrompt', 'worldInfoBefore', 'worldInfoAfter', 'dialogueExamples', 'chatHistory']
            .map(identifier => ({ identifier, enabled: true })) }];
    openai.getAdditionalParametersForSource(openai.oai_settings).include_headers = 'X-Outlet: {{outlet::projection}}';
    const textOf = messages => messages.flatMap(message => message.parts
        ? message.parts.map(part => part.text ?? '') : [message.content ?? '']).join('\n');

    for (const { note, position, interval } of [
        { note: 'User note', position: persona_description_positions.TOP_AN, interval: 1 },
        { note: '', position: persona_description_positions.BOTTOM_AN, interval: 1 },
        { note: 'User note', position: persona_description_positions.TOP_AN, interval: 3 },
    ]) {
        window.jQuery('#extension_floating_prompt').val(note);
        script.chat_metadata.note_interval = interval;
        power_user.persona_description_position = position;
        const captured = await buildAgentPromptSnapshotSeed({
            agentContextPolicy: { initialChatHistoryMessages: 0, includeActivatedWorldInfo: false },
            agentSystemPrompt: 'Inspect the captured inputs.',
        });
        const rootText = textOf(captured.currentPromptSnapshotSeed.messages);
        assert.equal(rootText.includes('WI '), false, rootText);
        assert.equal(rootText.includes('Recent chat'), false);
        assert.ok(rootText.includes('Character example'), rootText);
        const rootSnapshot = await materializeCurrentPromptSnapshot(captured);
        assert.equal(rootSnapshot.promptSnapshot.generationParameters.custom_include_headers, 'X-Outlet: ');
        const frozen = captured.frozenRunInputSnapshot;
        const original = JSON.stringify(frozen);

        for (const include of [true, false]) {
            const { promptSnapshot } = await buildPromptAssemblySnapshot({
                frozenRunInputSnapshot: frozen, settings: frozen.currentPreset.settings,
                agentContextPolicy: { initialChatHistoryMessages: include ? 1 : 0, includeActivatedWorldInfo: include },
                agentSystemPrompt: 'Inspect independently.',
            });
            const text = textOf(promptSnapshot.messages);
            for (const marker of ['WI before', 'WI after', 'WI example before', 'WI example after', 'WI depth', 'WI outlet', 'Recent chat']) {
                assert.equal(text.includes(marker), include, `${marker}: ${text}`);
            }
            assert.equal(text.includes('Older chat'), false);
            assert.ok(text.includes('Character example'));
            assert.equal(text.includes('User note'), interval === 1 && note !== '');
            assert.equal(text.includes('Persona note'), interval === 1);
            for (const marker of ['WI AN before', 'WI AN after']) {
                assert.equal(text.includes(marker), include && interval === 1, text);
            }
            if (include && interval === 1) {
                const noteParts = ['WI AN before', ...(note ? [note] : []), 'WI AN after'];
                position === persona_description_positions.TOP_AN ? noteParts.unshift('Persona note') : noteParts.push('Persona note');
                const offsets = noteParts.map(part => text.indexOf(part));
                assert.ok(offsets.every((offset, index) => index === 0 || offset > offsets[index - 1]), text);
            }
            assert.equal(promptSnapshot.generationParameters.custom_include_headers, `X-Outlet: ${include ? 'WI outlet' : ''}`);
        }
        assert.equal(JSON.stringify(frozen), original, 'Independent assembly must leave the captured sources intact');
        if (note && interval === 1) {
            let legacyPrompt;
            script.eventSource.once(script.event_types.GENERATE_AFTER_DATA, data => { legacyPrompt = data.prompt; });
            await script.Generate('normal', {}, true);
            assert.ok(textOf(legacyPrompt).includes('Persona note\nWI AN before\nUser note\nWI AN after'));
        }
    }
    console.log('PASS: captured sources support independent history, World Info, outlet and AN/Persona projection');
} finally {
    await window.happyDOM.close();
}
