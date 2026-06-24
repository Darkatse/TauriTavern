// @ts-check

import { OLED_READER_CHANGED_EVENT } from './constants.js';
import { applyEnabled, applyLevelVars, injectStyleOnce } from './dom.js';
import { clampLevel } from './palette.js';
import { loadState, saveState } from './state.js';
import { mountWidget, syncWidget } from './ui.js';

/**
 * Install the OLED red-black reader.
 *
 * Visuals are applied synchronously so the override class is on <html> before
 * the app's first paint (no flash when the mode was left enabled). The floating
 * control mounts once <body> is available.
 *
 * @returns {{ ready: Promise<void> }}
 */
export function installOledReader() {
    const state = loadState();

    injectStyleOnce();
    applyLevelVars(state.level);
    applyEnabled(state.enabled);

    const persistAndBroadcast = () => {
        saveState(state);
        window.dispatchEvent(new CustomEvent(OLED_READER_CHANGED_EVENT, {
            detail: { enabled: state.enabled, level: state.level },
        }));
    };

    /** @param {boolean} enabled */
    const setEnabled = (enabled) => {
        if (enabled === state.enabled) {
            return;
        }
        state.enabled = enabled;
        applyEnabled(enabled);
        syncWidget(state);
        persistAndBroadcast();
    };

    /** @param {number} level */
    const setLevel = (level) => {
        const next = clampLevel(level);
        if (next === state.level) {
            return;
        }
        state.level = next;
        applyLevelVars(next);
        persistAndBroadcast();
    };

    const handlers = {
        getState: () => ({ enabled: state.enabled, level: state.level }),
        onToggle: () => setEnabled(!state.enabled),
        onLevel: setLevel,
    };

    const mount = () => mountWidget(handlers);
    if (document.body) {
        mount();
    } else {
        document.addEventListener('DOMContentLoaded', mount, { once: true });
    }

    // Stay in sync if another surface changes the setting (guarded against echo).
    window.addEventListener(OLED_READER_CHANGED_EVENT, (event) => {
        const detail = /** @type {CustomEvent} */ (event).detail;
        if (!detail || typeof detail !== 'object') {
            return;
        }
        const record = /** @type {Record<string, unknown>} */ (detail);
        let changed = false;
        const nextEnabled = Boolean(record.enabled);
        if (nextEnabled !== state.enabled) {
            state.enabled = nextEnabled;
            applyEnabled(nextEnabled);
            changed = true;
        }
        const nextLevel = Number(record.level);
        if (Number.isFinite(nextLevel) && clampLevel(nextLevel) !== state.level) {
            state.level = clampLevel(nextLevel);
            applyLevelVars(state.level);
            changed = true;
        }
        if (changed) {
            syncWidget(state);
        }
    });

    return { ready: Promise.resolve() };
}
