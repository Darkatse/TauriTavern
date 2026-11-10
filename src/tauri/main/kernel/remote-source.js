// @ts-check

/** @param {string} src */
export function isHttpSource(src) {
    // DOM sources have normalized schemes; avoid parsing large data URL payloads.
    return src.startsWith('http:') || src.startsWith('https:');
}

/** @param {Window} targetWindow @param {URL} url */
export function isSameOrigin(targetWindow, url) {
    // srcdoc/about:blank have location.origin === 'null' but inherit their window origin.
    return url.origin === targetWindow.origin;
}

/**
 * Whether the host reads `src` rather than the page: the host fetches http(s) URLs on other origins
 * outside CORS, and only the page can read same-origin URLs, which the WebView serves.
 * @param {Window & typeof globalThis} targetWindow
 * @param {string} src
 */
export function isRemoteSource(targetWindow, src) {
    return isHttpSource(src) && !isSameOrigin(targetWindow, new targetWindow.URL(src));
}
