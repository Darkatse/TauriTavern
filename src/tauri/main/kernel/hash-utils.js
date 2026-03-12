// @ts-check

/**
 * @param {string} input
 * @returns {string}
 */
export function fnv1a32(input) {
    let hash = 0x811c9dc5;

    for (let index = 0; index < input.length; index += 1) {
        hash ^= input.charCodeAt(index);
        hash = (hash + ((hash << 1) + (hash << 4) + (hash << 7) + (hash << 8) + (hash << 24))) >>> 0;
    }

    return hash.toString(36);
}

