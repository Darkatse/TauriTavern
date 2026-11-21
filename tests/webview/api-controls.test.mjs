import { afterAll, expect, test } from '@rstest/core';
import { waitFor } from '@testing-library/dom';
import { computeAccessibleName } from 'dom-accessibility-api';
import { createFrontend } from '../helpers/frontend.mjs';

const { window, load, startHost, close } = await createFrontend();
await startHost();
await load('script.js');
const $ = window.jQuery;
const script = (await load('script.js'));
const kai = (await load('scripts/kai-settings.js'));
const textgen = (await load('scripts/textgen-settings.js'));
const power = (await load('scripts/power-user.js'));
kai.initKoboldSettings();
textgen.initTextGenSettings();

test('the global HTTP/2 keepalive control saves its interval and retains it when disabled', async () => {
    const frontend = await createFrontend();
    try {
        const { window, load, startHost } = frontend;
        let settings = { enabled: false, interval_seconds: 15 };
        const invoke = window.__TAURI__.core.invoke;
        window.__TAURI__.core.invoke = async (command, args, options) => {
            if (command === 'update_tauritavern_settings') {
                settings = { ...settings, ...args.dto.http2_keep_alive };
                return { http2_keep_alive: settings };
            }
            return invoke(command, args, options);
        };
        await startHost();
        const { initializeHttp2KeepAliveSettings } = await load('scripts/tauritavern/http2-keep-alive.js');
        initializeHttp2KeepAliveSettings(settings);

        const enabled = window.document.getElementById('http2_keep_alive_enabled');
        const interval = window.document.getElementById('http2_keep_alive_interval');
        expect(enabled.checked).toBe(false);
        expect(interval.disabled).toBe(true);

        enabled.click();
        await waitFor(() => expect(interval.disabled).toBe(false));
        interval.value = '28';
        interval.dispatchEvent(new window.Event('change', { bubbles: true }));
        await waitFor(() => expect(interval.disabled).toBe(false));
        expect(settings.interval_seconds).toBe(28);

        enabled.click();
        await waitFor(() => expect(enabled.disabled).toBe(false));
        expect(settings).toEqual({ enabled: false, interval_seconds: 28 });
        expect(interval.disabled).toBe(true);
        expect(interval.valueAsNumber).toBe(28);
    } finally {
        frontend.close();
    }
});

test('preset selection preserves legal context ranges and reports adjusted values once', async () => {
    power.prepareGenerationControls({ enableLabMode: false, max_context_unlocked: false });
    await power.loadPowerUserSettings({ power_user: { ...power.power_user } }, { themes: [], movingUIPresets: [], context: [] });
    const koboldPreset = { ...kai.kai_settings, temp: 1, genamt: 512, max_length: 65536, rep_pen_range: 32768 };
    const textgenPreset = { ...textgen.textgenerationwebui_settings, temp: 1, genamt: 512, max_length: 65536, rep_pen_range: 32768, dry_penalty_last_n: 32768 };
    kai.loadKoboldSettings({ koboldai_setting_names: ['Long'], koboldai_settings: [JSON.stringify(koboldPreset)] },
        { ...kai.kai_settings, preset_settings: 'Long' }, {});
    await textgen.loadTextGenSettings({ textgenerationwebui_preset_names: ['Long', 'Adjusted'], textgenerationwebui_presets: [
        JSON.stringify(textgenPreset), JSON.stringify({ ...textgenPreset, temp: 6, top_p: 2 }),
    ] }, { textgenerationwebui_settings: { ...textgen.textgenerationwebui_settings } });
    $('#settings_preset_textgenerationwebui').val('Long').trigger('change');
    expect(textgen.textgenerationwebui_settings.rep_pen_range).toBe(32768);
    expect(textgen.textgenerationwebui_settings.dry_penalty_last_n).toBe(32768);
    $('#max_context_unlocked').prop('checked', false).trigger('change');
    const presetsParent = $('#kobold_api-presets').parent();
    const parkedPresets = $('#kobold_api-presets').detach();
    try {
        parkedPresets.find('#settings_preset').val('0').trigger('change');
    } finally {
        presetsParent.prepend(parkedPresets);
    }
    expect(kai.kai_settings.preset_settings).toBe('Long');
    expect(kai.kai_settings.rep_pen_range).toBe(32768);
    expect(script.max_context).toBe(65536);
    expect($('.toast-warning').length).toBe(0);

    $('#settings_preset_textgenerationwebui').val('Adjusted').trigger('change');
    expect(textgen.textgenerationwebui_settings.temp).toBe(5);
    expect(textgen.textgenerationwebui_settings.top_p).toBe(1);
    expect($('.toast-warning').length).toBe(1);
    expect($('.toast-warning').text()).toMatch(/Text Completion/);
    expect($('.toast-warning').text()).toMatch(/6 → 5/);
    expect($('.toast-warning').text()).toMatch(/2 → 1/);
});

test('NovelAI settings saved before SillyTavern 1.13.2 restore their numeric strings as numbers', async () => {
    const nai = (await load('scripts/nai-settings.js'));
    nai.loadNovelSettings({ novelai_setting_names: [], novelai_settings: [] },
        { ...nai.nai_settings, top_k: '25', banned_tokens: '123' });
    expect(nai.nai_settings.top_k).toBe(25);
    expect(nai.nai_settings.banned_tokens).toBe('123');
});

test('Select2 keeps the field name and accepts native, generic and jQuery activation', () => {
    const source = $('#model_openrouter_select').empty().append('<option value="a">A</option><option value="b">B</option>');
    const name = computeAccessibleName(source[0], { hidden: true });
    source.select2();
    const trigger = source.next().find('.select2-selection')[0];
    expect(name && computeAccessibleName(trigger, { hidden: true }).includes(name)).toBeTruthy();
    for (const click of [element => element.click(), element => element.dispatchEvent(new window.Event('click', { bubbles: true })), element => $(element).trigger('click')]) {
        source.val('a').trigger('change');
        click(trigger);
        expect(source.data('select2').isOpen()).toBe(true);
        click($('.select2-results__option').filter((_, item) => item.textContent === 'B')[0]);
        expect(source.val()).toBe('b');
    }
    source.select2('destroy');
});

test('a saved key rejects on refresh failure and clears the input to avoid a duplicate write', async () => {
    const secrets = (await load('scripts/secrets.js'));
    $('#api_key_openai').val('TEST-ONLY');
    window.fetch = async url => {
        if (url === '/api/secrets/write') return window.Response.json({ id: 'saved' });
        expect(url).toBe('/api/secrets/read');
        return new window.Response('Test refresh failure', { status: 500 });
    };
    await expect(secrets.writeSecret('api_key_openai', 'TEST-ONLY', 'Test key')).rejects.toThrow(/refresh API key state/);
    expect($('#api_key_openai').val()).toBe('');
});

test('Connection Manager text completion uses quiet errors, profile api_type and host main-api availability', async () => {
    const { CONNECT_API_MAP, initDefaultSlashCommands } = (await load('scripts/slash-commands.js'));
    if (Object.keys(CONNECT_API_MAP).length === 0) {
        initDefaultSlashCommands();
    }
    const { ConnectionManagerRequestService } = (await load('scripts/extensions/shared.js'));
    const { TextCompletionService } = (await load('scripts/custom-request.js'));
    const { extension_settings } = (await load('scripts/extensions.js'));
    const previousFetch = window.fetch;
    const previousProfiles = extension_settings.connectionManager.profiles;
    const previousType = textgen.textgenerationwebui_settings.type;
    const previousOllamaModel = textgen.textgenerationwebui_settings.ollama_model;
    const previousIosPolicy = window.__TAURITAVERN__.iosPolicy;
    let seenBody;

    try {
        extension_settings.connectionManager.profiles = [
            { id: 'cm-tc', name: 'CM TC', mode: 'tc', api: 'koboldcpp', 'api-url': 'http://127.0.0.1:5001' },
            { id: 'cm-cc', name: 'CM CC', mode: 'cc', api: 'openai' },
        ];
        window.fetch = async (url, options) => {
            expect(url).toBe('/api/backends/text-completions/generate');
            seenBody = JSON.parse(options.body);
            return window.Response.json({ error: true, status: 401, response: 'Invalid API key' });
        };

        let thrown;
        try {
            await ConnectionManagerRequestService.sendRequest('cm-tc', [{ role: 'user', content: 'Hi' }], 16, { includePreset: false });
        } catch (error) {
            thrown = error;
        }
        expect(thrown?.cause?.message).toBe('Invalid API key');
        expect(seenBody?.type).toBe('quiet');

        textgen.textgenerationwebui_settings.type = textgen.textgen_types.OLLAMA;
        textgen.textgenerationwebui_settings.ollama_model = '';
        const payload = TextCompletionService.presetToGeneratePayload(
            { genamt: 32, temp: 0.5 },
            {},
            { api_type: textgen.textgen_types.KOBOLDCPP, api_server: 'http://127.0.0.1:5001', prompt: 'Hi' },
        );
        expect(payload.api_type).toBe(textgen.textgen_types.KOBOLDCPP);

        window.__TAURITAVERN__.iosPolicy = {
            scope: 'ios',
            profile: 'ios_external_beta',
            capabilities: { llm: { text_completions: { enabled: false } } },
            overridden_capabilities: [],
        };
        expect(ConnectionManagerRequestService.getSupportedProfiles().map(profile => profile.id)).toEqual(['cm-cc']);
    } finally {
        window.fetch = previousFetch;
        extension_settings.connectionManager.profiles = previousProfiles;
        textgen.textgenerationwebui_settings.type = previousType;
        textgen.textgenerationwebui_settings.ollama_model = previousOllamaModel;
        window.__TAURITAVERN__.iosPolicy = previousIosPolicy;
    }
});

afterAll(() => {
    script.cancelPendingSettingsSave();
    close();
});
