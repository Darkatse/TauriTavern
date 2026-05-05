// @ts-check

import { getTauriTavernSettings, updateTauriTavernSettings } from '../../../tauri-bridge.js';

const STORAGE_KEY = 'tt:nativeRegexBackendEnabled';

export const NATIVE_REGEX_BACKEND_SETTING_CHANGED_EVENT = 'tauritavern:native-regex-backend-enabled-changed';

/**
 * @param {Record<string, any> | null | undefined} settings
 * @returns {boolean}
 */
export function readNativeRegexBackendEnabledFromSettings(settings) {
    const value = settings?.native_regex_backend_enabled;
    return typeof value === 'boolean' ? value : true;
}

export function isNativeRegexBackendEnabled() {
    const value = globalThis.localStorage?.getItem(STORAGE_KEY);
    if (value === '0') {
        return false;
    }
    if (value === '1') {
        return true;
    }
    return true;
}

/**
 * @param {boolean} enabled
 * @returns {boolean}
 */
export function setNativeRegexBackendEnabled(enabled) {
    const normalized = Boolean(enabled);
    const previous = isNativeRegexBackendEnabled();
    globalThis.localStorage?.setItem(STORAGE_KEY, normalized ? '1' : '0');

    if (previous !== normalized && typeof window !== 'undefined') {
        window.dispatchEvent(new CustomEvent(NATIVE_REGEX_BACKEND_SETTING_CHANGED_EVENT, {
            detail: { enabled: normalized },
        }));
    }

    return normalized;
}

/**
 * @param {Record<string, any> | null | undefined} settings
 * @returns {boolean}
 */
export function syncNativeRegexBackendEnabledFromSettings(settings) {
    return setNativeRegexBackendEnabled(readNativeRegexBackendEnabledFromSettings(settings));
}

/**
 * @param {boolean} enabled
 * @returns {Promise<boolean>}
 */
export async function persistNativeRegexBackendEnabled(enabled) {
    const updatedSettings = await updateTauriTavernSettings({
        native_regex_backend_enabled: Boolean(enabled),
    });
    return syncNativeRegexBackendEnabledFromSettings(updatedSettings);
}

export function installNativeRegexBackendSetting() {
    const ready = getTauriTavernSettings().then(syncNativeRegexBackendEnabledFromSettings);
    return { ready };
}
