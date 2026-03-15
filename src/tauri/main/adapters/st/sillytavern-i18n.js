// @ts-check

/**
 * @param {string} key
 * @param {string} fallback
 * @returns {string}
 */
export function translateSillyTavern(key, fallback) {
    const hostWindow = /** @type {any} */ (window);
    const translate = hostWindow?.SillyTavern?.i18n?.translate;
    if (typeof translate !== 'function') {
        return fallback;
    }

    try {
        const translated = translate(fallback, key);
        if (typeof translated === 'string' && translated.trim()) {
            return translated;
        }
    } catch (error) {
        console.debug('Failed to translate notification text:', error);
    }

    return fallback;
}
