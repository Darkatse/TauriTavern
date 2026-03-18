// @ts-check

/**
 * @typedef {'auto' | 'off' | 'compat' | 'mobile-safe'} EmbeddedRuntimeProfileName
 */

export const EMBEDDED_RUNTIME_PROFILE_AUTO = 'auto';
export const EMBEDDED_RUNTIME_PROFILE_OFF = 'off';
export const EMBEDDED_RUNTIME_PROFILE_COMPAT = 'compat';
export const EMBEDDED_RUNTIME_PROFILE_MOBILE_SAFE = 'mobile-safe';

const EMBEDDED_RUNTIME_PROFILE_STORAGE_KEY = 'tt:embeddedRuntimeProfile';
const LEGACY_EMBEDDED_RUNTIME_PROFILE_STORAGE_KEY = 'tt:runtimeProfile';

/** @type {EmbeddedRuntimeProfileName | null} */
let cachedBootstrapProfileName = null;

/** @param {string} key */
function readStorageValue(key) {
    const raw = String(globalThis.localStorage?.getItem(key) || '').trim();
    return raw || null;
}

/**
 * @param {unknown} value
 * @returns {EmbeddedRuntimeProfileName | null}
 */
function normalizeLegacyEmbeddedRuntimeProfileName(value) {
    const profileName = String(value || '').trim();
    if (!profileName) {
        return null;
    }

    if (
        profileName === EMBEDDED_RUNTIME_PROFILE_COMPAT
        || profileName === EMBEDDED_RUNTIME_PROFILE_MOBILE_SAFE
    ) {
        return profileName;
    }

    throw new Error(`Unsupported legacy embedded runtime profile: ${profileName}`);
}

/**
 * @param {unknown} value
 * @returns {EmbeddedRuntimeProfileName}
 */
export function normalizeEmbeddedRuntimeProfileName(value) {
    const profileName = String(value || '').trim();
    if (!profileName) {
        throw new Error('Embedded runtime profile is required');
    }

    if (
        profileName === EMBEDDED_RUNTIME_PROFILE_AUTO
        || profileName === EMBEDDED_RUNTIME_PROFILE_OFF
        || profileName === EMBEDDED_RUNTIME_PROFILE_COMPAT
        || profileName === EMBEDDED_RUNTIME_PROFILE_MOBILE_SAFE
    ) {
        return profileName;
    }

    throw new Error(`Unsupported embedded runtime profile: ${profileName}`);
}

export function readStoredEmbeddedRuntimeProfileName() {
    const raw = readStorageValue(EMBEDDED_RUNTIME_PROFILE_STORAGE_KEY);
    return raw === null ? null : normalizeEmbeddedRuntimeProfileName(raw);
}

export function readLegacyEmbeddedRuntimeProfileName() {
    const raw = readStorageValue(LEGACY_EMBEDDED_RUNTIME_PROFILE_STORAGE_KEY);
    return raw === null ? null : normalizeLegacyEmbeddedRuntimeProfileName(raw);
}

export function getEmbeddedRuntimeBootstrapProfileName() {
    if (cachedBootstrapProfileName !== null) {
        return cachedBootstrapProfileName;
    }

    const stored = readStoredEmbeddedRuntimeProfileName();
    if (stored !== null) {
        cachedBootstrapProfileName = stored;
        return stored;
    }

    const legacy = readLegacyEmbeddedRuntimeProfileName();
    cachedBootstrapProfileName = legacy ?? EMBEDDED_RUNTIME_PROFILE_AUTO;
    return cachedBootstrapProfileName;
}

/**
 * @param {EmbeddedRuntimeProfileName} profileName
 * @returns {EmbeddedRuntimeProfileName}
 */
export function setEmbeddedRuntimeBootstrapProfileName(profileName) {
    const normalized = normalizeEmbeddedRuntimeProfileName(profileName);
    globalThis.localStorage?.setItem(EMBEDDED_RUNTIME_PROFILE_STORAGE_KEY, normalized);
    cachedBootstrapProfileName = normalized;
    return normalized;
}

export function clearLegacyEmbeddedRuntimeProfileName() {
    globalThis.localStorage?.removeItem(LEGACY_EMBEDDED_RUNTIME_PROFILE_STORAGE_KEY);
}

/**
 * @param {EmbeddedRuntimeProfileName} profileName
 * @returns {EmbeddedRuntimeProfileName}
 */
export function resolveEffectiveEmbeddedRuntimeProfileName(profileName) {
    const normalized = normalizeEmbeddedRuntimeProfileName(profileName);
    if (normalized !== EMBEDDED_RUNTIME_PROFILE_AUTO) {
        return normalized;
    }

    const stored = readStoredEmbeddedRuntimeProfileName();
    if (stored !== null) {
        return normalized;
    }

    return readLegacyEmbeddedRuntimeProfileName() ?? normalized;
}

export function isEmbeddedRuntimeTakeoverDisabled() {
    return getEmbeddedRuntimeBootstrapProfileName() === EMBEDDED_RUNTIME_PROFILE_OFF;
}
