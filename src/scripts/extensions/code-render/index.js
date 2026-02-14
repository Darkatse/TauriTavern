import {
    reloadCurrentChat,
    saveSettingsDebounced,
} from '../../../script.js';
import { extension_settings, renderExtensionTemplateAsync } from '../../extensions.js';
import { setHtmlCodeRenderEnabled } from '../../html-code-preview.js';

const MODULE_NAME = 'code-render';
const defaultSettings = {
    enabled: true,
};

function loadSettings() {
    if (!extension_settings.code_render || typeof extension_settings.code_render !== 'object') {
        extension_settings.code_render = { ...defaultSettings };
    }

    if (typeof extension_settings.code_render.enabled !== 'boolean') {
        extension_settings.code_render.enabled = defaultSettings.enabled;
    }

    setHtmlCodeRenderEnabled(extension_settings.code_render.enabled);
    $('#code_render_enabled').prop('checked', extension_settings.code_render.enabled);
}

async function onEnabledInput() {
    extension_settings.code_render.enabled = !!$('#code_render_enabled').prop('checked');
    setHtmlCodeRenderEnabled(extension_settings.code_render.enabled);
    saveSettingsDebounced();
    await reloadCurrentChat();
}

jQuery(async () => {
    const html = await renderExtensionTemplateAsync(MODULE_NAME, 'settings');
    $('#code_render_container').append(html);
    loadSettings();
    $('#code_render_enabled').on('input', onEnabledInput);
});
