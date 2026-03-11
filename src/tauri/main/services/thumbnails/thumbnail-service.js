// @ts-check

/**
 * @typedef {import('../../context/types.js').ThumbnailRouteSpec} ThumbnailRouteSpec
 * @typedef {import('../../context/types.js').ThumbnailBlobOptions} ThumbnailBlobOptions
 */

/**
 * @param {{
 *   buildThumbnailRouteUrl: (type: string, file: string, options?: { cacheBust?: string | number | null; animated?: boolean }) => string;
 *   parseThumbnailRouteUrl: (rawUrl: unknown) => ThumbnailRouteSpec | null;
 *   thumbnailRouteTypes: ReadonlySet<string>;
 *   imageThumbnailRouteTypes: ReadonlySet<string>;
 *   cacheLimit: number;
 * }} deps
 */
export function createThumbnailService({
    buildThumbnailRouteUrl,
    parseThumbnailRouteUrl,
    thumbnailRouteTypes,
    imageThumbnailRouteTypes,
    cacheLimit,
}) {
    const TRANSPARENT_PIXEL_DATA_URL = 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==';
    const THUMBNAIL_SRC_GUARD = Symbol('tauritavern-thumbnail-src-guard');
    const THUMBNAIL_REQUEST_TOKEN = Symbol('tauritavern-thumbnail-request-token');
    const normalizedLimit = Math.max(0, Math.floor(Number(cacheLimit) || 0));

    let thumbnailImageBridgeInstalled = false;
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

    function installThumbnailImageBridge() {
        if (thumbnailImageBridgeInstalled) {
            return;
        }

        const imagePrototype = window.HTMLImageElement?.prototype;
        if (!imagePrototype) {
            return;
        }

        const srcDescriptor = Object.getOwnPropertyDescriptor(imagePrototype, 'src');
        if (!srcDescriptor?.get || !srcDescriptor?.set || typeof imagePrototype.setAttribute !== 'function') {
            return;
        }

        const srcGet = /** @type {NonNullable<typeof srcDescriptor.get>} */ (srcDescriptor.get);
        const srcSet = /** @type {NonNullable<typeof srcDescriptor.set>} */ (srcDescriptor.set);
        const enumerable = Boolean(srcDescriptor.enumerable);

        const originalSetAttribute = imagePrototype.setAttribute;

        /** @param {HTMLImageElement} image @param {string} value */
        const setSourceDirectly = (image, value) => {
            const imageState = /** @type {any} */ (image);
            imageState[THUMBNAIL_SRC_GUARD] = true;
            try {
                srcSet.call(image, value);
            } finally {
                imageState[THUMBNAIL_SRC_GUARD] = false;
            }
        };

        /** @param {HTMLImageElement} image @param {any} rawValue */
        const handleThumbnailRouteSource = (image, rawValue) => {
            const value = String(rawValue ?? '');
            const parsed = parseThumbnailRouteUrl(value);
            if (!parsed || !imageThumbnailRouteTypes.has(parsed.type)) {
                setSourceDirectly(image, value);
                return;
            }

            const requestToken = `${parsed.type}|${parsed.file}|${parsed.cacheBust}|${Date.now()}|${Math.random()}`;
            /** @type {any} */ (image)[THUMBNAIL_REQUEST_TOKEN] = requestToken;
            setSourceDirectly(image, TRANSPARENT_PIXEL_DATA_URL);

            void resolveThumbnailBlobUrlFromSpec(parsed)
                .then((blobUrl) => {
                    if ((/** @type {any} */ (image))[THUMBNAIL_REQUEST_TOKEN] !== requestToken) {
                        return;
                    }
                    setSourceDirectly(image, blobUrl);
                })
                .catch(() => {
                    if ((/** @type {any} */ (image))[THUMBNAIL_REQUEST_TOKEN] !== requestToken) {
                        return;
                    }
                    setSourceDirectly(image, value);
                });
        };

        Object.defineProperty(imagePrototype, 'src', {
            configurable: true,
            enumerable,
            get() {
                return srcGet.call(this);
            },
            set(value) {
                if ((/** @type {any} */ (this))[THUMBNAIL_SRC_GUARD]) {
                    srcSet.call(this, value);
                    return;
                }
                handleThumbnailRouteSource(this, value);
            },
        });

        imagePrototype.setAttribute = function patchedImageSetAttribute(name, value) {
            if (String(name || '').toLowerCase() === 'src') {
                if ((/** @type {any} */ (this))[THUMBNAIL_SRC_GUARD]) {
                    return originalSetAttribute.call(this, name, value);
                }
                handleThumbnailRouteSource(this, value);
                return;
            }

            return originalSetAttribute.call(this, name, value);
        };

        thumbnailImageBridgeInstalled = true;
    }

    return {
        resolveThumbnailBlobUrl,
        installThumbnailImageBridge,
    };
}
