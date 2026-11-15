// @ts-check

const THUMBNAIL_TYPES = new Set(['bg', 'avatar', 'persona']);

/**
 * Keep `file` last: callers may read it from the URL's tail.
 * @param {string} type
 * @param {string} file
 * @param {{ list?: boolean, static?: boolean, useTimestamp?: boolean }} [options]
 * `list` ignores the original avatars setting; `static` serves a background's first frame.
 * @returns {string}
 */
export function buildThumbnailUrl(type, file, { list = false, static: staticPreview = false, useTimestamp = false } = {}) {
    const normalizedType = String(type || '').trim().toLowerCase();
    if (!THUMBNAIL_TYPES.has(normalizedType)) {
        throw new Error(`Unsupported thumbnail type: ${normalizedType}`);
    }

    const searchParams = new URLSearchParams({ type: normalizedType });
    if (list) {
        searchParams.set('list', 'true');
    }
    if (staticPreview) {
        searchParams.set('static', 'true');
    }
    if (useTimestamp) {
        searchParams.set('t', String(Date.now()));
    }
    searchParams.set('file', String(file || ''));
    return `/thumbnail?${searchParams.toString()}`;
}

/** @param {string} file */
function buildBackgroundPath(file) {
    return `/backgrounds/${encodeURIComponent(file)}`;
}

/** Public extension helpers must return stable, browser-loadable URLs. */
export function installAssetPathHelpers() {
    /**
     * @param {string} type
     * @param {string} file
     * @param {boolean} [useTimestamp]
     */
    window.__TAURITAVERN_THUMBNAIL__ = (type, file, useTimestamp = false) => buildThumbnailUrl(type, file, { useTimestamp });
    window.__TAURITAVERN_BACKGROUND_PATH__ = buildBackgroundPath;
}
