// @ts-check

import { invoke, isTauri } from '../../../tauri-bridge.js';

function getSafeInvoke() {
    if (typeof window === 'undefined') {
        return null;
    }

    const safeInvoke = window.__TAURITAVERN__?.invoke?.safeInvoke;
    return typeof safeInvoke === 'function' ? safeInvoke : null;
}

export function isNativeRegexBackendAvailable() {
    return isTauri();
}

/**
 * @param {{ tasks: { text: string; scripts: any[] }[] }} dto
 * @returns {Promise<{ tasks: { text: string }[] }>}
 */
export async function applyNativeRegexBatch(dto) {
    if (!isTauri()) {
        throw new Error('Native regex backend is only available in Tauri');
    }

    const safeInvoke = getSafeInvoke();
    if (safeInvoke) {
        return safeInvoke('apply_native_regex_batch', { dto });
    }

    return invoke('apply_native_regex_batch', { dto });
}
