// @ts-check

import { PanelRuntimeKind } from './panel-runtime-kinds.js';

/**
 * @typedef {import('../embedded-runtime/types.js').EmbeddedRuntimeProfile} EmbeddedRuntimeProfile
 */

const COMPAT_PROFILE = Object.freeze({
    name: 'compat',
    maxActiveWeight: 0,
    maxActiveIframes: 0,
    maxActiveSlots: 0,
    maxSoftParkedIframes: 0,
    softParkTtlMs: 0,
    parkWhenHiddenKinds: Object.freeze([
        PanelRuntimeKind.DrawerContent,
        PanelRuntimeKind.SubtreeGate,
    ]),
    rootMargin: '0px',
    threshold: 0,
});

const AGGRESSIVE_PROFILE = Object.freeze({
    name: 'aggressive',
    maxActiveWeight: 0,
    maxActiveIframes: 0,
    maxActiveSlots: 0,
    maxSoftParkedIframes: 0,
    softParkTtlMs: 0,
    parkWhenHiddenKinds: Object.freeze([
        PanelRuntimeKind.DrawerContent,
        PanelRuntimeKind.SubtreeGate,
    ]),
    rootMargin: '0px',
    threshold: 0,
});

/** @type {Readonly<Record<string, EmbeddedRuntimeProfile>>} */
export const PANEL_RUNTIME_PROFILES = Object.freeze({
    compat: COMPAT_PROFILE,
    aggressive: AGGRESSIVE_PROFILE,
});

/**
 * @param {string} profileName
 */
export function resolvePanelRuntimeProfile(profileName) {
    const name = String(profileName || '').trim();
    if (!name) {
        throw new Error('PanelRuntime: profile name is required');
    }

    const profile = PANEL_RUNTIME_PROFILES[name];
    if (!profile) {
        throw new Error(`PanelRuntime: unknown profile '${name}'`);
    }

    return profile;
}
