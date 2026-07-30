// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';

const CHAT_COMPLETION_API = 'openai';

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

function syncMainApiOptionParking() {
    const select = mustGetMainApiSelect();

    if (!parkedOptions) {
        parkedOptions = document.createDocumentFragment();
    }

    for (const option of Array.from(select.options)) {
        const value = String(option.value || '').trim();
        if (value === CHAT_COMPLETION_API) {
            continue;
        }

        parkedOptions.appendChild(option);
    }
}

/** @param {{ main_api?: string }} settings */
function selectChatCompletion(settings) {
    settings.main_api = CHAT_COMPLETION_API;
}

export function installMainApiOptionParking() {
    if (installed) {
        return;
    }
    installed = true;

    eventSource.on(event_types.SETTINGS_LOADED_BEFORE, selectChatCompletion);
    syncMainApiOptionParking();
}
