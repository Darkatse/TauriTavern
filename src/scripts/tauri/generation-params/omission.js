// @ts-check

import { REQUEST_PARAM_KEYS } from './catalog.js';

const NAMESPACE = 'tauritavern';
const FIELD = 'omit_params';

/**
 * Omitted parameter keys live in the preset's `extensions` slot so they follow
 * the preset through save / load / export. Presets without the field behave
 * exactly as before: every parameter is sent.
 *
 * @param {{ extensions?: Record<string, any> } | null | undefined} settings
 * @returns {string[]} Catalog keys currently omitted from requests
 */
export function getOmittedParams(settings) {
    const raw = settings?.extensions?.[NAMESPACE]?.[FIELD];
    if (!Array.isArray(raw)) {
        return [];
    }
    // Only catalog keys are honored, so a foreign preset cannot strip
    // structural fields such as `messages` or `model`.
    return [...new Set(raw.filter(key => REQUEST_PARAM_KEYS.has(key)))];
}

/**
 * @param {{ extensions?: Record<string, any> }} settings
 * @param {string} key
 * @param {boolean} omitted
 * @returns {boolean} Whether the settings changed
 */
export function setParamOmitted(settings, key, omitted) {
    if (!REQUEST_PARAM_KEYS.has(key)) {
        throw new Error(`Unknown request parameter: ${key}`);
    }
    const current = getOmittedParams(settings);
    const has = current.includes(key);
    if (has === omitted) {
        return false;
    }
    const next = omitted ? [...current, key] : current.filter(item => item !== key);

    settings.extensions ??= {};
    settings.extensions[NAMESPACE] ??= {};
    if (next.length) {
        settings.extensions[NAMESPACE][FIELD] = next;
    } else {
        delete settings.extensions[NAMESPACE][FIELD];
        if (!Object.keys(settings.extensions[NAMESPACE]).length) {
            delete settings.extensions[NAMESPACE];
        }
    }
    return true;
}

/**
 * Remove omitted keys from a request payload. Runs after all upstream
 * per-source shaping so the payload owner's decisions stay intact.
 *
 * @template {Record<string, any>} T
 * @param {T} generateData
 * @param {{ extensions?: Record<string, any> } | null | undefined} settings
 * @returns {T}
 */
export function applyParamOmissions(generateData, settings) {
    for (const key of getOmittedParams(settings)) {
        delete generateData[key];
    }
    return generateData;
}
