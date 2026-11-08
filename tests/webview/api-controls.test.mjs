import { afterAll, expect, test } from '@rstest/core';
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
    $('#settings_preset').val('0').trigger('change');
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

afterAll(() => {
    script.cancelPendingSettingsSave();
    close();
});
