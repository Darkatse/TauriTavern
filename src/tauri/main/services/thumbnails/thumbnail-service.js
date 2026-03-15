// @ts-check

/**
 * @typedef {import('../../context/types.js').ThumbnailRouteSpec} ThumbnailRouteSpec
 * @typedef {import('../../context/types.js').ThumbnailBlobOptions} ThumbnailBlobOptions
 */

/**
 * @param {{
 *   buildThumbnailRouteUrl: (type: string, file: string, options?: { cacheBust?: string | number | null; animated?: boolean }) => string;
 *   thumbnailRouteTypes: ReadonlySet<string>;
 *   cacheLimit: number;
 * }} deps
 */
export function createThumbnailService({
    buildThumbnailRouteUrl,
    thumbnailRouteTypes,
    cacheLimit,
}) {
    const normalizedLimit = Math.max(0, Math.floor(Number(cacheLimit) || 0));

    /** @type {Map<string, string>} */
    const thumbnailBlobCache = new Map();
    /** @type {Map<string, Promise<string>>} */
    const thumbnailBlobInFlight = new Map();

    /**
     * @param {string} type
     * @param {string} file
     * @param {boolean} animated
     */
    function makeThumbnailBlobCachePrefix(type, file, animated) {
        return `${type}|${animated ? 1 : 0}|${encodeURIComponent(file)}|`;
    }

    /**
     * @param {string} type
     * @param {string} file
     * @param {boolean} animated
     * @param {string} cacheBust
     */
    function makeThumbnailBlobCacheKey(type, file, animated, cacheBust) {
        return `${makeThumbnailBlobCachePrefix(type, file, animated)}${cacheBust || ''}`;
    }

    /**
     * @param {string} cacheKey
     * @param {string} blobUrl
     */
    function setThumbnailBlobCache(cacheKey, blobUrl) {
        if (thumbnailBlobCache.has(cacheKey)) {
            const previousBlobUrl = thumbnailBlobCache.get(cacheKey);
            if (previousBlobUrl && previousBlobUrl !== blobUrl) {
                URL.revokeObjectURL(previousBlobUrl);
            }
            thumbnailBlobCache.delete(cacheKey);
        }

        thumbnailBlobCache.set(cacheKey, blobUrl);

        if (normalizedLimit <= 0 || thumbnailBlobCache.size <= normalizedLimit) {
            return;
        }

        const oldestKey = /** @type {string} */ (thumbnailBlobCache.keys().next().value);
        const oldestBlobUrl = thumbnailBlobCache.get(oldestKey);
        if (oldestBlobUrl) {
            URL.revokeObjectURL(oldestBlobUrl);
        }
        thumbnailBlobCache.delete(oldestKey);
    }

    /**
     * @param {string} type
     * @param {string} file
     * @param {boolean} animated
     */
    function invalidateThumbnailBlobCache(type, file, animated) {
        const prefix = makeThumbnailBlobCachePrefix(type, file, animated);

        for (const [key, blobUrl] of thumbnailBlobCache.entries()) {
            if (!key.startsWith(prefix)) {
                continue;
            }

            if (blobUrl) {
                URL.revokeObjectURL(blobUrl);
            }
            thumbnailBlobCache.delete(key);
        }

        for (const key of thumbnailBlobInFlight.keys()) {
            if (key.startsWith(prefix)) {
                thumbnailBlobInFlight.delete(key);
            }
        }
    }

    /**
     * @param {Partial<ThumbnailRouteSpec> | undefined} spec
     * @returns {Promise<string>}
     */
    async function resolveThumbnailBlobUrlFromSpec({ type, file, animated = false, cacheBust = '' } = {}) {
        const normalizedType = String(type || '').trim().toLowerCase();
        const normalizedFile = String(file || '').trim();

        if (!thumbnailRouteTypes.has(normalizedType) || !normalizedFile) {
            throw new Error(`Unsupported thumbnail request: ${normalizedType}`);
        }

        const normalizedAnimated = Boolean(animated);
        const normalizedCacheBust = String(cacheBust || '').trim();
        if (normalizedCacheBust) {
            invalidateThumbnailBlobCache(normalizedType, normalizedFile, normalizedAnimated);
        }

        const cacheKey = makeThumbnailBlobCacheKey(
            normalizedType,
            normalizedFile,
            normalizedAnimated,
            normalizedCacheBust,
        );

        const cachedBlobUrl = thumbnailBlobCache.get(cacheKey);
        if (cachedBlobUrl) {
            thumbnailBlobCache.delete(cacheKey);
            thumbnailBlobCache.set(cacheKey, cachedBlobUrl);
            return cachedBlobUrl;
        }

        const inflight = thumbnailBlobInFlight.get(cacheKey);
        if (inflight) {
            return inflight;
        }

        const requestUrl = buildThumbnailRouteUrl(normalizedType, normalizedFile, {
            animated: normalizedAnimated,
            cacheBust: normalizedCacheBust || null,
        });

        const fetchPromise = fetch(requestUrl, { cache: 'no-store' })
            .then(async (response) => {
                if (!response.ok) {
                    throw new Error(`Failed to load thumbnail: ${response.status}`);
                }

                const blob = await response.blob();
                const blobUrl = URL.createObjectURL(blob);
                setThumbnailBlobCache(cacheKey, blobUrl);
                return blobUrl;
            })
            .finally(() => {
                thumbnailBlobInFlight.delete(cacheKey);
            });

        thumbnailBlobInFlight.set(cacheKey, fetchPromise);
        return fetchPromise;
    }

    /**
     * @param {string} type
     * @param {string} file
     * @param {ThumbnailBlobOptions | undefined} options
     */
    async function resolveThumbnailBlobUrl(type, file, options = {}) {
        const cacheBust = options?.useTimestamp ? String(Date.now()) : '';
        return resolveThumbnailBlobUrlFromSpec({
            type,
            file,
            animated: Boolean(options?.animated),
            cacheBust,
        });
    }

    return {
        resolveThumbnailBlobUrl,
    };
}
