// @ts-check

import { DEFAULT_ENABLED, DEFAULT_LEVEL, STORAGE_KEY } from './constants.js';
import { clampLevel } from './palette.js';

/**
 * @typedef {object} OledReaderState
 * @property {boolean} enabled
 * @property {number} level
 */

/** @returns {OledReaderState} */
function defaultState() {
    return { enabled: DEFAULT_ENABLED, level: DEFAULT_LEVEL };
}

/**
 * Load persisted reader state from localStorage, falling back to defaults.
 * @returns {OledReaderState}
 */
export function loadState() {
    try {
        const raw = window.localStorage?.getItem(STORAGE_KEY);
        if (!raw) {
            return defaultState();
        }
        const parsed = /** @type {unknown} */ (JSON.parse(raw));
        if (!parsed || typeof parsed !== 'object') {
            return defaultState();
        }
        const record = /** @type {Record<string, unknown>} */ (parsed);
        const levelRaw = Number(record.level);
        return {
            enabled: Boolean(record.enabled),
            level: Number.isFinite(levelRaw) ? clampLevel(levelRaw) : DEFAULT_LEVEL,
        };
    } catch (error) {
        console.warn('TauriTavern OLED reader: failed to read saved state', error);
        return defaultState();
    }
}

/**
 * Persist reader state to localStorage.
 * @param {OledReaderState} state
 */
export function saveState(state) {
    try {
        const payload = JSON.stringify({ enabled: Boolean(state.enabled), level: clampLevel(state.level) });
        window.localStorage?.setItem(STORAGE_KEY, payload);
    } catch (error) {
        console.warn('TauriTavern OLED reader: failed to persist state', error);
    }
}
