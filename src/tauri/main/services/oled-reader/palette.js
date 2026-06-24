// @ts-check

import { MAX_LEVEL, MIN_LEVEL } from './constants.js';

/**
 * Clamp a red level into the readable range.
 * @param {number} level
 * @returns {number}
 */
export function clampLevel(level) {
    if (!Number.isFinite(level)) {
        return MIN_LEVEL;
    }
    return Math.min(MAX_LEVEL, Math.max(MIN_LEVEL, level));
}

/**
 * @typedef {object} OledPalette
 * @property {number} level   clamped level (0.15..1)
 * @property {number} channel red channel value (99..255)
 * @property {string} red     rgb() string for primary text/icons
 * @property {string} dimRed  rgb() string for secondary info (same hue, ~55%)
 */

/**
 * Compute the red-on-black palette from a single level value.
 *
 * Only the red channel moves; green and blue stay 0 so just the red subpixel
 * lights up. Secondary info uses the same hue at ~55% so it never falls back to
 * grey (which would introduce blue/green light).
 *
 * @param {number} level
 * @returns {OledPalette}
 */
export function computePalette(level) {
    const clamped = clampLevel(level);
    const channel = Math.min(255, Math.max(72, Math.round(72 + 183 * clamped)));
    const dim = Math.round(channel * 0.55);
    return {
        level: clamped,
        channel,
        red: `rgb(${channel}, 0, 0)`,
        dimRed: `rgb(${dim}, 0, 0)`,
    };
}
