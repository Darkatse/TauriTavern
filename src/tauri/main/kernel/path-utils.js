// @ts-check

/**
 * @param {string | null | undefined} base
 * @param {string | null | undefined} child
 */
export function joinPath(base, child) {
    if (!base) {
        return null;
    }

    const normalizedBase = String(base).replace(/[\\/]+$/, '');
    const normalizedChild = String(child || '').replace(/^[\\/]+/, '');
    const separator = normalizedBase.includes('\\') ? '\\' : '/';

    return `${normalizedBase}${separator}${normalizedChild.replace(/[\\/]+/g, separator)}`;
}

/**
 * @param {string} file
 */
export function sanitizeRelativePath(file) {
    const decoded = decodeURIComponent(String(file));
    const normalized = decoded.replace(/[\\/]+/g, '/').replace(/^\/+/, '');

    if (!normalized || normalized.includes('..')) {
        return null;
    }

    return normalized;
}

