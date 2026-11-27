// @ts-check

import { translateSillyTavern } from './sillytavern-i18n.js';

/**
 * @typedef {object} HostAvailability
 * @property {(keys: unknown) => void} install
 * @property {(value: unknown) => boolean} isAvailable
 * @property {(value: string) => string} resolve
 */

/**
 * @param {object} options
 * @param {string} options.selectId
 * @param {(value: string) => string} options.keyOf
 * @param {string} options.noticeKey
 * @param {string} options.notice
 * @returns {HostAvailability}
 */
function createHostAvailability({ selectId, keyOf, noticeKey, notice }) {
    /** @type {'all' | [string, ...string[]] | null} */
    let availableKeys = null;
    /** @type {Map<string, string>} */
    const removedLabels = new Map();

    /** @returns {'all' | [string, ...string[]]} */
    function mustGetAvailableKeys() {
        if (availableKeys === null) {
            throw new Error(`HostAvailability(#${selectId}): host keys are not installed`);
        }
        return availableKeys;
    }

    /** @param {unknown} value */
    function isAvailable(value) {
        const keys = mustGetAvailableKeys();
        return typeof value === 'string' && (keys === 'all' || keys.includes(keyOf(value.trim())));
    }

    /** @param {string} value */
    function resolve(value) {
        if (isAvailable(value)) {
            return value;
        }

        /** @type {any} */ (window).toastr.warning(
            translateSillyTavern(noticeKey, notice),
            removedLabels.get(value) ?? value,
        );
        // Only a key list can leave a value unavailable.
        return /** @type {[string, ...string[]]} */ (availableKeys)[0];
    }

    /** @param {unknown} keys */
    function install(keys) {
        if (availableKeys !== null) {
            throw new Error(`HostAvailability(#${selectId}): already installed`);
        }
        if (keys !== 'all' && !isNonEmptyKeyList(keys)) {
            throw new Error(`HostAvailability(#${selectId}): unsupported key list ${JSON.stringify(keys)}`);
        }
        availableKeys = keys;

        const select = document.getElementById(selectId);
        if (!(select instanceof HTMLSelectElement)) {
            throw new Error(`HostAvailability(#${selectId}): <select> not found`);
        }
        for (const option of Array.from(select.options)) {
            if (!isAvailable(option.value)) {
                removedLabels.set(option.value, option.text);
                option.remove();
            }
        }
    }

    return { install, isAvailable, resolve };
}

/**
 * @param {unknown} keys
 * @returns {keys is [string, ...string[]]}
 */
function isNonEmptyKeyList(keys) {
    return Array.isArray(keys) && keys.length > 0 && keys.every(key => typeof key === 'string');
}

export const chatCompletionSources = createHostAvailability({
    selectId: 'chat_completion_source',
    keyOf: value => value.startsWith('custom_') ? 'custom' : value,
    noticeKey: 'tauritavern_chat_completion_source_unavailable',
    notice: 'This source is not available. Another source has been selected.',
});

export const textCompletionTypes = createHostAvailability({
    selectId: 'textgen_type',
    keyOf: value => value,
    noticeKey: 'tauritavern_text_completion_type_unavailable',
    notice: 'This API type is not available. Another API type has been selected.',
});
