export function isAndroidRuntime() {
    return typeof navigator !== 'undefined'
        && typeof navigator.userAgent === 'string'
        && /android/i.test(navigator.userAgent);
}

export function isIosRuntime() {
    if (typeof navigator === 'undefined') {
        return false;
    }

    const userAgent = typeof navigator.userAgent === 'string' ? navigator.userAgent : '';
    if (/iphone|ipad|ipod/i.test(userAgent)) {
        return true;
    }

    const touchPoints = Number(navigator.maxTouchPoints || 0);
    if (touchPoints <= 1) {
        return false;
    }

    const platform = typeof navigator.platform === 'string' ? navigator.platform : '';
    return platform === 'MacIntel' || /macintosh/i.test(userAgent);
}
