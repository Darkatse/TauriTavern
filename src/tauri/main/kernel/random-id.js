// @ts-check

/**
 * IDs the page picks for Rust requests and streams; they use only letters, digits and `-`.
 * @returns {string}
 */
export function createRandomId() {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
        return crypto.randomUUID();
    }

    const timestamp = Date.now().toString(36);
    const random = Math.random().toString(36).slice(2, 10);
    return `${timestamp}-${random}`;
}
