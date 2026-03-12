// @ts-check

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

function readProfileOverride() {
    const raw = String(globalThis.localStorage?.getItem('tt:runtimeProfile') || '').trim();
    return raw || null;
}

/** @type {EmbeddedRuntimeProfile} */
const COMPAT_PROFILE = Object.freeze({
    name: 'compat',
    maxActiveWeight: 200,
    maxActiveIframes: 12,
    maxActiveSlots: 80,
    parkWhenHiddenKinds: Object.freeze([
        EmbeddedRuntimeKind.JsrHtmlRender,
    ]),
    rootMargin: '300px 0px',
    threshold: 0.01,
});

/** @type {EmbeddedRuntimeProfile} */
const MOBILE_SAFE_PROFILE = Object.freeze({
    name: 'mobile-safe',
    maxActiveWeight: 80,
    maxActiveIframes: 4,
    maxActiveSlots: 30,
    parkWhenHiddenKinds: Object.freeze([
        EmbeddedRuntimeKind.JsrHtmlRender,
    ]),
    rootMargin: '500px 0px',
    threshold: 0.01,
});

export const EMBEDDED_RUNTIME_PROFILES = Object.freeze({
    compat: COMPAT_PROFILE,
    'mobile-safe': MOBILE_SAFE_PROFILE,
});

export function resolveEmbeddedRuntimeProfile() {
    const override = readProfileOverride();
    if (override === 'compat') {
        return COMPAT_PROFILE;
    }
    if (override === 'mobile-safe') {
        return MOBILE_SAFE_PROFILE;
    }

    if (isMobileUserAgent()) {
        return MOBILE_SAFE_PROFILE;
    }

    return COMPAT_PROFILE;
}
