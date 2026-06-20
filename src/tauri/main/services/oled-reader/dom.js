// @ts-check

import { CSS_CHANNEL, CSS_RED, CSS_RED_DIM, ROOT_CLASS, STYLE_ELEMENT_ID } from './constants.js';
import { computePalette } from './palette.js';
import { buildOledCss } from './style.js';

/** Inject the override stylesheet once into <head>. */
export function injectStyleOnce() {
    if (document.getElementById(STYLE_ELEMENT_ID)) {
        return;
    }
    const style = document.createElement('style');
    style.id = STYLE_ELEMENT_ID;
    style.textContent = buildOledCss();
    (document.head || document.documentElement).appendChild(style);
}

/**
 * Push the computed palette into inline custom properties on <html>.
 * @param {number} level
 */
export function applyLevelVars(level) {
    const palette = computePalette(level);
    const root = document.documentElement;
    root.style.setProperty(CSS_RED, palette.red);
    root.style.setProperty(CSS_RED_DIM, palette.dimRed);
    root.style.setProperty(CSS_CHANNEL, String(palette.channel));
}

/**
 * Toggle reader mode on the root element and keep the mobile chrome color
 * (meta theme-color) in sync.
 * @param {boolean} enabled
 */
export function applyEnabled(enabled) {
    document.documentElement.classList.toggle(ROOT_CLASS, enabled);
    syncMetaThemeColor(enabled);
}

/** @param {boolean} enabled */
function syncMetaThemeColor(enabled) {
    const meta = document.querySelector('meta[name="theme-color"]');
    if (!(meta instanceof HTMLMetaElement)) {
        return;
    }
    if (enabled) {
        if (meta.dataset.ttOledPrev === undefined) {
            meta.dataset.ttOledPrev = meta.content;
        }
        meta.content = '#000000';
        return;
    }
    if (meta.dataset.ttOledPrev !== undefined) {
        meta.content = meta.dataset.ttOledPrev;
        delete meta.dataset.ttOledPrev;
    }
}
