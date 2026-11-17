// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';
import { getActiveIosPolicyCapabilities } from '../../../../scripts/tauritavern/ios-policy.js';
import { translateSillyTavern } from './sillytavern-i18n.js';

const CHAT_COMPLETION_API = 'openai';
const TEXT_COMPLETION_API = 'textgenerationwebui';

/** @type {boolean} */
let installed = false;

/** @type {DocumentFragment | null} */
let parkedOptions = null;

/**
 * @returns {HTMLSelectElement}
 */
function mustGetMainApiSelect() {
    const el = document.getElementById('main_api');
    if (!(el instanceof HTMLSelectElement)) {
        throw new Error('MainApiOptionParking: #main_api <select> not found');
    }
    return el;
}

/**
 * The main APIs the host implements; iOS policy can turn text completion off.
 * @returns {Set<string>}
 */
function getAvailableMainApis() {
    const available = new Set([CHAT_COMPLETION_API, TEXT_COMPLETION_API]);
    if (getActiveIosPolicyCapabilities()?.llm?.text_completions?.enabled === false) {
        available.delete(TEXT_COMPLETION_API);
    }
    return available;
}

/**
 * @param {Set<string>} available
 */
function parkUnavailableOptions(available) {
    const select = mustGetMainApiSelect();
    parkedOptions ??= document.createDocumentFragment();

    for (const option of Array.from(select.options)) {
        if (!available.has(option.value.trim())) {
            parkedOptions.appendChild(option);
        }
    }
}

/**
 * @param {string} api
 * @returns {string}
 */
function getMainApiLabel(api) {
    const option = parkedOptions?.querySelector(`option[value="${CSS.escape(api)}"]`);
    return option?.textContent?.trim() || api;
}

/** @param {{ main_api?: unknown }} settings */
function selectAvailableMainApi(settings) {
    const available = syncMainApiOptionParking();
    const saved = typeof settings.main_api === 'string' ? settings.main_api.trim() : '';
    if (saved && available.has(saved)) {
        return;
    }

    settings.main_api = CHAT_COMPLETION_API;
    if (saved) {
        /** @type {any} */ (window).toastr.warning(
            translateSillyTavern(
                'tauritavern_main_api_unavailable',
                'This API is not available. Switched to Chat Completion.',
            ),
            getMainApiLabel(saved),
        );
    }
}

/**
 * @param {unknown} api
 * @returns {boolean}
 */
export function isMainApiAvailable(api) {
    return typeof api === 'string' && getAvailableMainApis().has(api.trim());
}

export function syncMainApiOptionParking() {
    const available = getAvailableMainApis();
    parkUnavailableOptions(available);
    return available;
}

export function installMainApiOptionParking() {
    if (installed) {
        return;
    }
    installed = true;

    eventSource.on(event_types.SETTINGS_LOADED_BEFORE, selectAvailableMainApi);
    syncMainApiOptionParking();
}
