// NOTE: Intentionally self-contained UA checks.
// This module is used by Tauri chat/runtime code and must stay dependency-free to avoid import-order
// constraints or cycles. Do not "deduplicate" by importing a shared helper unless it is guaranteed
// side-effect-free and available in every runtime we ship.

export function isAndroidRuntime() {
    return typeof navigator !== 'undefined'
        && typeof navigator.userAgent === 'string'
        && /android/i.test(navigator.userAgent);
}

export function isIOSRuntime() {
    return typeof navigator !== 'undefined'
        && typeof navigator.userAgent === 'string'
        && /iphone|ipad|ipod/i.test(navigator.userAgent);
}

export function isMobileRuntime() {
    return isAndroidRuntime() || isIOSRuntime();
}
