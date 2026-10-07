import assert from 'node:assert/strict';
import test from 'node:test';
import { computeAccessibleName } from 'dom-accessibility-api';
import { createBrowserRuntime } from './runtime.mjs';

test('API presets, Select2 activation and key failures preserve their public behavior', async context => {
    const { window, load, getModule, startHost } = createBrowserRuntime();
    try {
        await startHost();
        await load('script.js');
        await load('scripts/select2-accessibility.js');
        const $ = window.jQuery;
        const script = getModule('script.js').namespace;
        const kai = getModule('scripts/kai-settings.js').namespace;
        const textgen = getModule('scripts/textgen-settings.js').namespace;
        const power = getModule('scripts/power-user.js').namespace;
        kai.initKoboldSettings();
        textgen.initTextGenSettings();

        await context.test('preset selection preserves legal context ranges and reports adjusted values once', async () => {
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
            assert.equal(textgen.textgenerationwebui_settings.rep_pen_range, 32768);
            assert.equal(textgen.textgenerationwebui_settings.dry_penalty_last_n, 32768);
            $('#max_context_unlocked').prop('checked', false).trigger('change');
            $('#settings_preset').val('0').trigger('change');
            assert.equal(kai.kai_settings.rep_pen_range, 32768);
            assert.equal(script.max_context, 65536);
            assert.equal($('.toast-warning').length, 0);

            $('#settings_preset_textgenerationwebui').val('Adjusted').trigger('change');
            assert.equal(textgen.textgenerationwebui_settings.temp, 5);
            assert.equal(textgen.textgenerationwebui_settings.top_p, 1);
            assert.equal($('.toast-warning').length, 1);
            assert.match($('.toast-warning').text(), /Text Completion/);
            assert.match($('.toast-warning').text(), /6 → 5/);
            assert.match($('.toast-warning').text(), /2 → 1/);
        });

        await context.test('NovelAI settings saved before SillyTavern 1.13.2 restore their numeric strings as numbers', () => {
            const nai = getModule('scripts/nai-settings.js').namespace;
            nai.loadNovelSettings({ novelai_setting_names: [], novelai_settings: [] },
                { ...nai.nai_settings, top_k: '25', banned_tokens: '123' });
            assert.equal(nai.nai_settings.top_k, 25);
            assert.equal(nai.nai_settings.banned_tokens, '123');
        });

        await context.test('Select2 keeps the field name and accepts native, generic and jQuery activation', () => {
            const source = $('#model_openrouter_select').empty().append('<option value="a">A</option><option value="b">B</option>');
            const name = computeAccessibleName(source[0], { hidden: true });
            source.select2();
            const trigger = source.next().find('.select2-selection')[0];
            assert.ok(name && computeAccessibleName(trigger, { hidden: true }).includes(name));
            for (const click of [element => element.click(), element => element.dispatchEvent(new window.Event('click', { bubbles: true })), element => $(element).trigger('click')]) {
                source.val('a').trigger('change');
                click(trigger);
                assert.equal(source.data('select2').isOpen(), true);
                click($('.select2-results__option').filter((_, item) => item.textContent === 'B')[0]);
                assert.equal(source.val(), 'b');
            }
            source.select2('destroy');
        });

        await context.test('a saved key rejects on refresh failure and clears the input to avoid a duplicate write', async () => {
            const secrets = getModule('scripts/secrets.js').namespace;
            $('#api_key_openai').val('TEST-ONLY');
            window.fetch = async url => {
                if (url === '/api/secrets/write') return window.Response.json({ id: 'saved' });
                assert.equal(url, '/api/secrets/read');
                return new window.Response('Test refresh failure', { status: 500 });
            };
            await assert.rejects(secrets.writeSecret('api_key_openai', 'TEST-ONLY', 'Test key'), /refresh API key state/);
            assert.equal($('#api_key_openai').val(), '');
        });
    } finally {
        getModule('script.js').namespace.cancelPendingSettingsSave();
        await window.happyDOM.close();
    }
});
