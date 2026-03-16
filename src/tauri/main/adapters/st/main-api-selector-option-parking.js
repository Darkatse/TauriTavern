// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';

const ALLOWED_MAIN_APIS = new Set(['openai', 'textgenerationwebui']);

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
    const current = String(select.value || '').trim();

    if (!parkedOptions) {
        parkedOptions = document.createDocumentFragment();
    }

    for (const option of Array.from(select.options)) {
        const value = String(option.value || '').trim();
        if (!value) {
            continue;
        }
        if (ALLOWED_MAIN_APIS.has(value)) {
            continue;
        }
        if (value === current) {
            continue;
        }

        parkedOptions.appendChild(option);
    }
}

export function installMainApiOptionParking() {
    if (installed) {
        return;
    }
    installed = true;

    eventSource.on(event_types.SETTINGS_LOADED, syncMainApiOptionParking);
    eventSource.on(event_types.MAIN_API_CHANGED, syncMainApiOptionParking);
}

