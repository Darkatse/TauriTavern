const thumbnailCacheBustTokens = new Map();

function makeThumbnailCacheKey(type, file) {
    const normalizedType = String(type || '').trim().toLowerCase();
    const normalizedFile = String(file || '');

    if (!normalizedType || !normalizedFile) {
        return null;
    }

    return `${normalizedType}\u0000${normalizedFile}`;
}

export function bumpThumbnailCacheBust(type, file, token = Date.now()) {
    const key = makeThumbnailCacheKey(type, file);

    if (!key) {
        throw new Error('Invalid thumbnail cache-bust key');
    }

    const normalizedToken = String(token);
    thumbnailCacheBustTokens.set(key, normalizedToken);
    return normalizedToken;
}

export function getThumbnailCacheBust(type, file) {
    const key = makeThumbnailCacheKey(type, file);

    if (!key) {
        return null;
    }

    return thumbnailCacheBustTokens.get(key) ?? null;
}
