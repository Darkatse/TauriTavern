// @ts-check

/**
 * @typedef {{ start: number; end: number }} ProjectionRange
 * @typedef {{
 *   indices: readonly number[];
 *   ranges: readonly ProjectionRange[];
 * }} ChatProjection
 */

/**
 * @param {readonly number[]} indices
 * @returns {ProjectionRange[]}
 */
function collectRanges(indices) {
    /** @type {ProjectionRange[]} */
    const ranges = [];
    for (const index of indices) {
        const last = ranges[ranges.length - 1];
        if (last && index === last.end) {
            last.end += 1;
        } else {
            ranges.push({ start: index, end: index + 1 });
        }
    }
    return ranges;
}

/**
 * Validates a committed projection without silently sorting, deduplicating or
 * clamping it. Callers must make their intended message set explicit.
 *
 * @param {readonly number[]} indices
 * @param {{ count: number }} options
 * @returns {ChatProjection}
 */
export function createChatProjection(indices, { count }) {
    if (!Array.isArray(indices)) {
        throw new TypeError('ChatSurface projection indices must be an array');
    }
    if (!Number.isInteger(count) || count < 0) {
        throw new TypeError('ChatSurface projection count must be a non-negative integer');
    }
    let previous = -1;
    for (const index of indices) {
        if (!Number.isInteger(index)) {
            throw new TypeError(`ChatSurface projection index must be an integer: ${String(index)}`);
        }
        if (index < 0 || index >= count) {
            throw new RangeError(`ChatSurface projection index ${index} is outside [0, ${count})`);
        }
        if (index <= previous) {
            throw new Error('ChatSurface projection indices must be unique and strictly increasing');
        }
        previous = index;
    }

    const frozenIndices = Object.freeze([...indices]);
    const ranges = collectRanges(frozenIndices).map(range => Object.freeze(range));
    if (ranges.length > 2) {
        throw new Error(`ChatSurface projection has ${ranges.length} ranges; maximum is 2`);
    }

    return Object.freeze({
        indices: frozenIndices,
        ranges: Object.freeze(ranges),
    });
}
