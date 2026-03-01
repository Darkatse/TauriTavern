export function isAndroidRuntime() {
    return typeof navigator !== 'undefined'
        && typeof navigator.userAgent === 'string'
        && /android/i.test(navigator.userAgent);
}

