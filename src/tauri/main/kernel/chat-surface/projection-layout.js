// @ts-check

import { createChatProjection } from './projection.js';

/** @param {unknown} value @param {string} label */
function requireFiniteNonNegative(value, label) {
    if (!Number.isFinite(value) || Number(value) < 0) {
        throw new Error(`ChatSurface ${label} must be finite and non-negative`);
    }
    return Number(value);
}

/** @param {readonly any[]} items @param {number} count @param {string} label */
function validateItems(items, count, label) {
    if (!Array.isArray(items)) {
        throw new TypeError(`ChatSurface ${label} must be an array`);
    }
    let previousIndex = -1;
    let previousEnd = -Infinity;
    for (const item of items) {
        if (!item || !Number.isInteger(item.index) || item.index < 0 || item.index >= count) {
            throw new Error(`ChatSurface ${label} contains an invalid index`);
        }
        if (item.index <= previousIndex) {
            throw new Error(`ChatSurface ${label} indices must be unique and strictly increasing`);
        }
        const start = requireFiniteNonNegative(item.start, `${label} start`);
        const end = requireFiniteNonNegative(item.end, `${label} end`);
        if (end < start || start < previousEnd) {
            throw new Error(`ChatSurface ${label} geometry is inverted or overlapping`);
        }
        previousIndex = item.index;
        previousEnd = end;
    }
}

/** @param {number} height @param {boolean} present */
function spacer(height, present) {
    if (!present) {
        return Object.freeze({ present: false, height: 0 });
    }
    return Object.freeze({
        present: true,
        height: requireFiniteNonNegative(height, 'spacer height'),
    });
}

/**
 * Converts measured virtual items into the only bounded DOM layout accepted by
 * ChatSurface: one contiguous viewport range plus the canonical true tail.
 *
 * @param {{
 *   count: number;
 *   viewportItems: readonly any[];
 *   projectedItems: readonly any[];
 *   paddingStart?: number;
 *   gap?: number;
 *   maxViewportItems: number;
 * }} input
 */
export function createBoundedProjectionLayout({
    count,
    viewportItems,
    projectedItems,
    paddingStart = 0,
    gap = 0,
    maxViewportItems,
}) {
    if (!Number.isInteger(count) || count < 0) {
        throw new TypeError('ChatSurface layout count must be a non-negative integer');
    }
    if (!Number.isInteger(maxViewportItems) || maxViewportItems < 1) {
        throw new TypeError('ChatSurface maxViewportItems must be a positive integer');
    }
    const startPadding = requireFiniteNonNegative(paddingStart, 'padding start');
    const rowGap = requireFiniteNonNegative(gap, 'row gap');
    validateItems(viewportItems, count, 'viewport items');
    validateItems(projectedItems, count, 'projected items');

    if (viewportItems.length > maxViewportItems) {
        throw new Error(`ChatSurface viewport exceeds Vmax=${maxViewportItems}`);
    }
    for (let index = 1; index < viewportItems.length; index += 1) {
        if (viewportItems[index].index !== viewportItems[index - 1].index + 1) {
            throw new Error('ChatSurface viewport items must form one contiguous range');
        }
    }

    if (count === 0) {
        if (viewportItems.length !== 0 || projectedItems.length !== 0) {
            throw new Error('Empty ChatSurface layout cannot contain virtual items');
        }
        return Object.freeze({
            projection: createChatProjection([], { count }),
            viewportMessageIds: Object.freeze([]),
            tailMessageId: null,
            topSpacer: spacer(0, false),
            middleSpacer: spacer(0, false),
        });
    }

    const tailMessageId = count - 1;
    const projectedByIndex = new Map(projectedItems.map(item => [item.index, item]));
    const tail = projectedByIndex.get(tailMessageId);
    if (!tail) {
        throw new Error('Bounded ChatSurface projection must contain the canonical true tail');
    }

    const viewportMessageIds = viewportItems.map(item => item.index);
    const expectedIds = new Set([...viewportMessageIds, tailMessageId]);
    if (
        projectedItems.length !== expectedIds.size
        || projectedItems.some(item => !expectedIds.has(item.index))
    ) {
        throw new Error('Bounded ChatSurface projection must equal V ∪ T');
    }

    const projection = createChatProjection(projectedItems.map(item => item.index), { count });

    if (viewportItems.length === 0) {
        const hasTopSpacer = tailMessageId > 0;
        return Object.freeze({
            projection,
            viewportMessageIds: Object.freeze(viewportMessageIds),
            tailMessageId,
            topSpacer: spacer(tail.start - startPadding - rowGap, hasTopSpacer),
            middleSpacer: spacer(0, false),
        });
    }

    const firstViewport = viewportItems[0];
    const lastViewport = viewportItems[viewportItems.length - 1];
    const hasTopSpacer = firstViewport.index > 0;
    const tailSeparated = lastViewport.index < tailMessageId - 1;
    return Object.freeze({
        projection,
        viewportMessageIds: Object.freeze(viewportMessageIds),
        tailMessageId,
        topSpacer: spacer(firstViewport.start - startPadding - rowGap, hasTopSpacer),
        middleSpacer: spacer(tail.start - lastViewport.end - (2 * rowGap), tailSeparated),
    });
}
