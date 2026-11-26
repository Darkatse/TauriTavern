// @ts-check

import { translateSillyTavern } from './sillytavern-i18n.js';

/** @type {'all' | [string, ...string[]] | null} */
let availableSources = null;

/** @returns {'all' | [string, ...string[]]} */
function mustGetAvailableSources() {
    if (availableSources === null) {
        throw new Error('ChatCompletionSourceAvailability: host sources are not installed');
    }
    return availableSources;
}

/**
 * @param {string} value
 * @returns {string}
 */
function getSourceKey(value) {
    const key = value.trim();
    return key.startsWith('custom_') ? 'custom' : key;
}

/**
 * @param {unknown} value
 * @returns {boolean}
 */
export function isChatCompletionSourceAvailable(value) {
    const sources = mustGetAvailableSources();
    return typeof value === 'string' && (sources === 'all' || sources.includes(getSourceKey(value)));
}

/**
 * @param {string} value
 * @param {string} label
 * @returns {string}
 */
export function resolveAvailableChatCompletionSource(value, label) {
    if (isChatCompletionSourceAvailable(value)) {
        return value;
    }

    /** @type {any} */ (window).toastr.warning(
        translateSillyTavern(
            'tauritavern_chat_completion_source_unavailable',
            'This source is not available. Another source has been selected.',
        ),
        label,
    );
    // Only a source list can leave a value unavailable.
    return /** @type {[string, ...string[]]} */ (availableSources)[0];
}

/**
 * @param {unknown} sources
 * @returns {sources is [string, ...string[]]}
 */
function isNonEmptySourceList(sources) {
    return Array.isArray(sources) && sources.length > 0 && sources.every(source => typeof source === 'string');
}

/** @param {unknown} sources */
export function installChatCompletionSourceAvailability(sources) {
    if (availableSources !== null) {
        throw new Error('ChatCompletionSourceAvailability: already installed');
    }
    if (sources !== 'all' && !isNonEmptySourceList(sources)) {
        throw new Error(`ChatCompletionSourceAvailability: unsupported source list ${JSON.stringify(sources)}`);
    }
    availableSources = sources;

    const select = document.getElementById('chat_completion_source');
    if (!(select instanceof HTMLSelectElement)) {
        throw new Error('ChatCompletionSourceAvailability: #chat_completion_source <select> not found');
    }
    for (const option of Array.from(select.options)) {
        if (!isChatCompletionSourceAvailable(option.value)) {
            option.remove();
        }
    }
}
