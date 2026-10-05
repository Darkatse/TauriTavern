const ENABLED_STORAGE_KEY = 'ttPluginBridge.enabled';
const BASE_URL_STORAGE_KEY = 'ttPluginBridge.baseUrl';
const AUTH_STORAGE_KEY = 'ttPluginBridge.auth';
const DEFAULT_BASE_URL = 'http://127.0.0.1:8000';

function readStorageValue(key) {
    try {
        return window.localStorage?.getItem(key) ?? null;
    } catch {
        return null;
    }
}

/**
 * Runtime config for the server plugin bridge (routes/plugin-proxy-routes.js).
 *
 * These settings intentionally live in localStorage: the host address is a
 * per-device deployment fact (desktop talks to 127.0.0.1, mobile needs the
 * desktop's LAN address), while tauritavern-settings.json is synced verbatim
 * between devices by TT-Sync and would clobber the per-device value.
 *   'ttPluginBridge.enabled' = 'false' disables the bridge (default: enabled)
 *   'ttPluginBridge.baseUrl' overrides the target (default: http://127.0.0.1:8000)
 *   'ttPluginBridge.auth' = 'user:password' sends Basic auth credentials
 *     (value of config.yaml basicAuthUser). The proxy cannot observe the
 *     browser's native Basic-auth cache, so a host with basicAuthMode: true
 *     needs this.
 *
 * @returns {{enabled: boolean, baseUrl: string}}
 */
export function readBridgeConfig() {
    const enabledRaw = (readStorageValue(ENABLED_STORAGE_KEY) ?? '').trim().toLowerCase();
    const enabled = enabledRaw !== 'false' && enabledRaw !== '0' && enabledRaw !== 'off';
    const baseUrl = (readStorageValue(BASE_URL_STORAGE_KEY) ?? DEFAULT_BASE_URL).trim().replace(/\/+$/, '');
    return {
        enabled: enabled && baseUrl.length > 0,
        baseUrl,
    };
}

/**
 * @returns {string} Trimmed 'user:password' credential pair, empty when unset.
 */
export function readBridgeCredentials() {
    return (readStorageValue(AUTH_STORAGE_KEY) ?? '').trim();
}
