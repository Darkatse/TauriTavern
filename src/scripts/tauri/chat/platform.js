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
