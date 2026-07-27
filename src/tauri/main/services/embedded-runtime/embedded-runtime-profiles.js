// @ts-check

import {
    EMBEDDED_RUNTIME_PROFILE_AUTO,
    EMBEDDED_RUNTIME_PROFILE_COMPAT,
    EMBEDDED_RUNTIME_PROFILE_MOBILE_SAFE,
    EMBEDDED_RUNTIME_PROFILE_OFF,
    normalizeEmbeddedRuntimeProfileName,
} from './embedded-runtime-profile-state.js';
import { EmbeddedRuntimeKind } from './runtime-kinds.js';

/**
 * @typedef {import('./types.js').EmbeddedRuntimeProfile} EmbeddedRuntimeProfile
 */

function isMobileUserAgent() {
    const userAgent = typeof navigator?.userAgent === 'string' ? navigator.userAgent : '';
    if (/android|iphone|ipad|ipod/i.test(userAgent)) {
        return true;
    }

    return navigator?.platform === 'MacIntel' && navigator?.maxTouchPoints > 1;
}

/** @type {EmbeddedRuntimeProfile} */
const COMPAT_PROFILE = Object.freeze({
    name: 'compat',
    maxActiveWeight: 200,
    maxActiveIframes: 18,
    maxActiveSlots: 80,
    maxSoftParkedIframes: 36,
    softParkTtlMs: 120_000,
    parkWhenHiddenKinds: Object.freeze([
        EmbeddedRuntimeKind.JsrHtmlRender,
        EmbeddedRuntimeKind.LittleWhiteBoxHtmlRender,
    ]),
    rootMargin: '400px 0px',
    threshold: 0,
});

/** @type {EmbeddedRuntimeProfile} */
const MOBILE_SAFE_PROFILE = Object.freeze({
    name: 'mobile-safe',
    maxActiveWeight: 80,
    maxActiveIframes: 8,
    maxActiveSlots: 30,
    maxSoftParkedIframes: 16,
    softParkTtlMs: 45_000,
    parkWhenHiddenKinds: Object.freeze([
        EmbeddedRuntimeKind.JsrHtmlRender,
        EmbeddedRuntimeKind.LittleWhiteBoxHtmlRender,
    ]),
    rootMargin: '900px 0px',
    threshold: 0,
});

export const EMBEDDED_RUNTIME_PROFILES = Object.freeze({
    compat: COMPAT_PROFILE,
    'mobile-safe': MOBILE_SAFE_PROFILE,
});

/** @param {string} profileName */
export function resolveEmbeddedRuntimeProfile(profileName) {
    const normalized = normalizeEmbeddedRuntimeProfileName(profileName);
    if (normalized === EMBEDDED_RUNTIME_PROFILE_OFF) {
        throw new Error('Embedded runtime profile "off" does not resolve to a runtime manager profile');
    }

    if (normalized === EMBEDDED_RUNTIME_PROFILE_COMPAT) {
        return COMPAT_PROFILE;
    }

    if (normalized === EMBEDDED_RUNTIME_PROFILE_MOBILE_SAFE) {
        return MOBILE_SAFE_PROFILE;
    }

    if (normalized !== EMBEDDED_RUNTIME_PROFILE_AUTO) {
        throw new Error(`Unsupported embedded runtime profile: ${normalized}`);
    }

    if (isMobileUserAgent()) {
        return MOBILE_SAFE_PROFILE;
    }

    return COMPAT_PROFILE;
}
