import { DOMPurify } from '../../../lib.js';
import { event_types, eventSource } from '../../../script.js';
import { t } from '../../i18n.js';
import {
    chat_completion_sources,
    getChatCompletionModel,
    getChatCompletionModelControl,
    oai_settings,
    openai_setting_names,
    openai_settings,
    settingsToUpdate,
} from '../../openai.js';
import { Popup, POPUP_RESULT, POPUP_TYPE } from '../../popup.js';
import { createPresetConnectionKeeper } from './preset-connection.js';

/** Preset key → settings key, for the keys that describe the connection (fourth flag). */
const CONNECTION_PRESET_KEYS = new Map(Object.entries(settingsToUpdate)
    .filter(([, [, , , isConnection]]) => isConnection)
    .map(([presetKey, [, settingKey]]) => [presetKey, settingKey]));

const DROP_CONNECTION_RESULT = POPUP_RESULT.CUSTOM1;

/**
 * Stored body of a Chat Completion preset.
 * @param {string} name Preset name
 * @returns {Record<string, any>|null}
 */
function getStoredPreset(name) {
    const index = openai_setting_names[name];
    return index === undefined ? null : openai_settings[index] ?? null;
}

/**
 * Stored body of the loaded Chat Completion preset, or null when its name no longer
 * resolves to a stored preset.
 * @returns {Record<string, any>|null}
 */
export function getLoadedPresetBody() {
    return getStoredPreset(oai_settings.preset_settings_openai);
}

/**
 * Where requests go: the source, that source's model, a custom endpoint and the reverse
 * proxy. The other connection keys (models remembered for other sources, a custom URL
 * kept while another source is active, …) do not change it.
 * @param {Record<string, any>} settings Chat Completion settings
 * @returns {string}
 */
function readRoute(settings) {
    const isCustom = settings.chat_completion_source === chat_completion_sources.CUSTOM;
    return JSON.stringify([
        settings.chat_completion_source,
        getChatCompletionModel(settings),
        isCustom ? settings.custom_url : null,
        isCustom ? settings.custom_api_format : null,
        settings.reverse_proxy,
    ]);
}

/**
 * The live settings as they would be after loading this preset with its connection.
 * @param {Record<string, unknown>} preset Preset body
 * @returns {Record<string, any>}
 */
function settingsWithPresetConnection(preset) {
    const next = { ...oai_settings };
    for (const [presetKey, settingKey] of CONNECTION_PRESET_KEYS) {
        if (Object.hasOwn(preset, presetKey)) {
            next[settingKey] = preset[presetKey];
        }
    }
    return next;
}

/**
 * While the selected item owns the connection (a saved model, or a profile that records
 * the API), it decides the connection: loading a preset leaves the connection alone, and
 * saving a preset keeps the connection the preset already stored instead of absorbing
 * the selected one. Otherwise the upstream "bind presets to connections" toggle decides
 * as before, and importing a preset that would switch the connection asks first.
 * @param {() => boolean} ownsConnection Whether the selected item owns the connection
 * @returns {() => void} Brings the upstream binding toggle in line with the current ownership
 */
export function installConnectionOwnership(ownsConnection) {
    const bind = document.getElementById('bind_preset_to_connection');
    if (!(bind instanceof HTMLInputElement)) {
        throw new Error('Connection Manager: #bind_preset_to_connection not found');
    }
    const keeper = createPresetConnectionKeeper(CONNECTION_PRESET_KEYS.keys());

    eventSource.on(event_types.OAI_PRESET_CHANGED_BEFORE, (event) => {
        if (ownsConnection()) {
            event.bindConnection = false;
        }
    });

    eventSource.on(event_types.PRESET_RENAMED_BEFORE, ({ apiId, oldName, newName }) => {
        if (apiId === 'openai') {
            keeper.renaming(newName, getStoredPreset(oldName));
        }
    });

    eventSource.on(event_types.OAI_PRESET_SAVE_BEFORE, ({ name, preset, previous }) => {
        keeper.saving({ name, preset, previous, loaded: getLoadedPresetBody(), owned: ownsConnection() });
    });

    eventSource.on(event_types.OAI_PRESET_IMPORT_READY, async ({ data }) => {
        // Only the case where the import is about to switch the user's model is worth a question.
        if (ownsConnection() || !oai_settings.bind_preset_to_connection) {
            return;
        }
        const next = settingsWithPresetConnection(data);
        if (readRoute(next) === readRoute(oai_settings)) {
            return;
        }
        const source = getChatCompletionModelControl(next.chat_completion_source)?.label ?? next.chat_completion_source;
        const model = getChatCompletionModel(next);
        const target = DOMPurify.sanitize([source, model].filter(Boolean).join(' · '));
        // Escape keeps the upstream behavior: load it as is.
        const popup = new Popup(
            `<h3>${t`This preset also stores a connection`}</h3><p>${t`Loading it will switch your current API or model.`}</p><p><b>${target}</b></p>`,
            POPUP_TYPE.TEXT,
            '',
            {
                okButton: t`Load and switch connection`,
                cancelButton: false,
                customButtons: [{ text: t`Import preset only (drop connection)`, result: DROP_CONNECTION_RESULT, classes: ['popup-button-ok'] }],
            },
        );
        if (await popup.show() !== DROP_CONNECTION_RESULT) {
            return;
        }
        for (const presetKey of CONNECTION_PRESET_KEYS.keys()) {
            delete data[presetKey];
        }
    });

    return () => {
        const owned = ownsConnection();
        bind.disabled = owned;
        bind.title = owned ? t`A selected model decides the connection` : '';
    };
}
