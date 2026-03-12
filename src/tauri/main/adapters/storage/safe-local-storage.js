// @ts-check

/**
 * @returns {Storage | null}
 */
export function getLocalStorage() {
    try {
        return window?.localStorage ?? null;
    } catch {
        return null;
    }
}

