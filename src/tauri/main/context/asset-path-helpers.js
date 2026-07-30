// @ts-check

/**
 * Installs global path helper functions that third-party scripts/extensions call directly.
 *
 * These helpers must stay stable and return browser-loadable URLs.
 *
 * @param {{
 *   thumbnailRouteTypes: ReadonlySet<string>;
 * }} deps
 */
export function installAssetPathHelpers({
    thumbnailRouteTypes,
}) {
    /**
     * @param {string} type
     * @param {string} file
     * @param {boolean} [useTimestamp]
     */
    function buildThumbnailUrl(type, file, useTimestamp = false) {
        const normalizedType = String(type || '').trim().toLowerCase();
        if (!thumbnailRouteTypes.has(normalizedType)) {
            throw new Error(`Unsupported thumbnail type: ${normalizedType}`);
        }

        const searchParams = new URLSearchParams({
            type: normalizedType,
            file: String(file || ''),
        });
        if (useTimestamp) {
            searchParams.set('t', String(Date.now()));
        }
        return `/thumbnail?${searchParams.toString()}`;
    }

    /** @param {string} file */
    function buildBackgroundPath(file) {
        return `/backgrounds/${encodeURIComponent(file)}`;
    }

    window.__TAURITAVERN_THUMBNAIL__ = buildThumbnailUrl;
    window.__TAURITAVERN_BACKGROUND_PATH__ = buildBackgroundPath;

    return {
        buildThumbnailUrl,
        buildBackgroundPath,
    };
}
