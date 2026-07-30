// @ts-check

/** @param {HTMLElement} target @param {HTMLElement} source */
export function syncElementAttributes(target, source) {
    for (const name of target.getAttributeNames()) {
        if (!source.hasAttribute(name)) {
            target.removeAttribute(name);
        }
    }
    for (const name of source.getAttributeNames()) {
        const value = source.getAttribute(name);
        if (value === null) {
            throw new Error(`Chat message staged attribute disappeared: ${name}`);
        }
        target.setAttribute(name, value);
    }
}
