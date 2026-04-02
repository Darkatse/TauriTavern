// @ts-check

/**
 * @typedef {import('./types.js').ThumbnailBlobOptions} ThumbnailBlobOptions
 */

/**
 * Installs global path helper functions that third-party scripts/extensions call directly.
 *
 * These helpers must stay stable and return browser-loadable URLs.
 *
 * @param {{
 *   assetService: {
 *     buildThumbnailRouteUrl: (type: string, file: string, options?: any) => string;
 *     resolveAssetPath: (type: string, file: string) => string | null;
 *     toAssetUrl: (path: string) => string | null;
 *   };
 *   thumbnailService: {
 *     resolveThumbnailBlobUrl: (
 *       type: string,
 *       file: string,
 *       options?: ThumbnailBlobOptions,
 *     ) => Promise<string>;
 *   };
 *   thumbnailRouteTypes: ReadonlySet<string>;
 * }} deps
 */
export function installAssetPathHelpers({
    assetService,
    thumbnailService,
    thumbnailRouteTypes,
}) {
    /**
     * @param {string} type
     * @param {string} file
     * @param {boolean} [useTimestamp]
     */
    function buildThumbnailUrl(type, file, useTimestamp = false) {
        const normalizedType = String(type || '').trim().toLowerCase();

        if (thumbnailRouteTypes.has(normalizedType)) {
            return assetService.buildThumbnailRouteUrl(normalizedType, file, {
                cacheBust: useTimestamp ? Date.now() : null,
            });
        }

        const filePath = assetService.resolveAssetPath(normalizedType, file);

        if (filePath) {
            const assetUrl = assetService.toAssetUrl(filePath);
            if (assetUrl) {
                return `${assetUrl}${useTimestamp ? `?t=${Date.now()}` : ''}`;
            }
        }

        return assetService.buildThumbnailRouteUrl(normalizedType, file);
    }

    /** @param {string} file */
    function buildBackgroundPath(file) {
        return `/backgrounds/${encodeURIComponent(file)}`;
    }

    /** @param {string} file */
    function buildAvatarPath(file) {
        const filePath = assetService.resolveAssetPath('avatar', file);
        const assetUrl = filePath ? assetService.toAssetUrl(filePath) : null;
        return assetUrl || null;
    }

    /** @param {string} file */
    function buildPersonaPath(file) {
        const filePath = assetService.resolveAssetPath('persona', file);
        const assetUrl = filePath ? assetService.toAssetUrl(filePath) : null;
        return assetUrl || `User Avatars/${file}`;
    }

    /**
     * @param {string} type
     * @param {string} file
     * @param {ThumbnailBlobOptions} [options]
     */
    function resolveThumbnailBlobUrl(type, file, options = {}) {
        return thumbnailService.resolveThumbnailBlobUrl(type, file, options);
    }

    window.__TAURITAVERN_THUMBNAIL__ = buildThumbnailUrl;
    window.__TAURITAVERN_BACKGROUND_PATH__ = buildBackgroundPath;
    window.__TAURITAVERN_AVATAR_PATH__ = buildAvatarPath;
    window.__TAURITAVERN_PERSONA_PATH__ = buildPersonaPath;
    window.__TAURITAVERN_THUMBNAIL_BLOB_URL__ = resolveThumbnailBlobUrl;

    return {
        buildThumbnailUrl,
        buildBackgroundPath,
        buildAvatarPath,
        buildPersonaPath,
        resolveThumbnailBlobUrl,
    };
}
