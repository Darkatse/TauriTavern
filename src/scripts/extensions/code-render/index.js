import {
    reloadCurrentChat,
    saveSettingsDebounced,
} from '../../../script.js';
import { extension_settings, renderExtensionTemplateAsync } from '../../extensions.js';
import {
    setHtmlCodeRenderEnabled,
    setHtmlCodeRenderReplaceLastMessageByDefault,
} from '../../html-code-preview.js';

const MODULE_NAME = 'code-render';
const defaultSettings = {
    enabled: false,
    replace_last_message_by_default: false,
};

function loadSettings() {
    if (!extension_settings.code_render || typeof extension_settings.code_render !== 'object') {
        extension_settings.code_render = { ...defaultSettings };
    }

    if (typeof extension_settings.code_render.enabled !== 'boolean') {
        extension_settings.code_render.enabled = defaultSettings.enabled;
    }

    if (typeof extension_settings.code_render.replace_last_message_by_default !== 'boolean') {
        extension_settings.code_render.replace_last_message_by_default = defaultSettings.replace_last_message_by_default;
    }

    setHtmlCodeRenderEnabled(extension_settings.code_render.enabled);
    setHtmlCodeRenderReplaceLastMessageByDefault(extension_settings.code_render.replace_last_message_by_default);
    $('#code_render_enabled').prop('checked', extension_settings.code_render.enabled);
    $('#code_render_replace_last_message_by_default').prop('checked', extension_settings.code_render.replace_last_message_by_default);
}

async function onSettingsInput() {
    extension_settings.code_render.enabled = !!$('#code_render_enabled').prop('checked');
    extension_settings.code_render.replace_last_message_by_default = !!$('#code_render_replace_last_message_by_default').prop('checked');
    setHtmlCodeRenderEnabled(extension_settings.code_render.enabled);
    setHtmlCodeRenderReplaceLastMessageByDefault(extension_settings.code_render.replace_last_message_by_default);
    saveSettingsDebounced();
    await reloadCurrentChat();
}

jQuery(async () => {
    const html = await renderExtensionTemplateAsync(MODULE_NAME, 'settings');
    $('#code_render_container').append(html);
    loadSettings();
    $('#code_render_enabled').on('input', onSettingsInput);
    $('#code_render_replace_last_message_by_default').on('input', onSettingsInput);
});
