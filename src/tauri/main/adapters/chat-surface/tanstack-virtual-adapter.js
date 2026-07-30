// @ts-check

import {
    CHAT_VIRTUAL_ESTIMATE_PX,
    CHAT_VIRTUAL_MAX_VIEWPORT_ITEMS,
    CHAT_VIRTUAL_OVERSCAN,
    CHAT_VIRTUAL_SCROLL_END_THRESHOLD_PX,
    CHAT_VIRTUAL_SCROLL_RESET_DELAY_MS,
} from '../../kernel/chat-surface/virtualization-config.js';

/** @param {unknown} value @param {string} label */
function requirePixelValue(value, label) {
    const number = Number.parseFloat(String(value ?? '0'));
    if (!Number.isFinite(number) || number < 0) {
        throw new Error(`Chat virtualizer ${label} must be a non-negative pixel value`);
    }
    return number;
}

/** @param {HTMLElement} root */
function readLayoutMetrics(root) {
    const style = getComputedStyle(root);
    return Object.freeze({
        paddingStart: requirePixelValue(style.paddingBlockStart || style.paddingTop, 'padding start'),
        paddingEnd: requirePixelValue(style.paddingBlockEnd || style.paddingBottom, 'padding end'),
        gap: style.rowGap === 'normal' ? 0 : requirePixelValue(style.rowGap, 'row gap'),
    });
}

/** @param {HTMLElement} element @param {ResizeObserverEntry | undefined} entry */
function measureMessage(element, entry) {
    const borderBox = entry?.borderBoxSize?.[0];
    const blockSize = borderBox
        ? borderBox.blockSize
        : element.getBoundingClientRect().height;
    const style = getComputedStyle(element);
    return requirePixelValue(blockSize, 'message block size')
        + requirePixelValue(style.marginBlockStart || style.marginTop, 'message margin start')
        + requirePixelValue(style.marginBlockEnd || style.marginBottom, 'message margin end');
}

/** @param {number} value @param {number} min @param {number} max */
function clamp(value, min, max) {
    return Math.min(Math.max(value, min), max);
}

/** @param {number} count @param {number} center @param {number} maxItems */
function windowAround(count, center, maxItems) {
    if (count === 0) {
        return [];
    }
    const length = Math.min(count, maxItems);
    const start = clamp(
        center - Math.floor(length / 2),
        0,
        count - length,
    );
    return Array.from({ length }, (_value, offset) => start + offset);
}

/** @param {{ startIndex: number; endIndex: number; overscan: number; count: number }} range @param {number} maxItems @param {(range: any) => number[]} extract */
function cappedViewportRange(range, maxItems, extract) {
    const extracted = extract(range);
    if (extracted.length <= maxItems) {
        return extracted;
    }
    const center = Math.floor((range.startIndex + range.endIndex) / 2);
    return windowAround(range.count, center, maxItems);
}

/**
 * The only module allowed to touch TanStack's framework-adapter lifecycle.
 * ChatSurface consumes immutable geometry snapshots only.
 *
 * @param {{
 *   root: HTMLElement;
 *   onGeometryChange: (change: { scrolling: boolean; programmatic: boolean }) => void;
 *   virtualCore: {
 *     Virtualizer: typeof import('@tanstack/virtual-core').Virtualizer;
 *     defaultRangeExtractor: typeof import('@tanstack/virtual-core').defaultRangeExtractor;
 *     observeElementOffset: typeof import('@tanstack/virtual-core').observeElementOffset;
 *     observeElementRect: typeof import('@tanstack/virtual-core').observeElementRect;
 *   };
 *   scrollToFn: import('@tanstack/virtual-core').VirtualizerOptions<HTMLElement, HTMLElement>['scrollToFn'];
 *   maxViewportItems?: number;
 * }} deps
 */
export function createTanStackVirtualAdapter({
    root,
    onGeometryChange,
    virtualCore,
    scrollToFn,
    maxViewportItems = CHAT_VIRTUAL_MAX_VIEWPORT_ITEMS,
}) {
    if (!(root instanceof HTMLElement)) {
        throw new TypeError('Chat virtualizer requires an HTMLElement scroll root');
    }
    if (typeof onGeometryChange !== 'function') {
        throw new TypeError('Chat virtualizer requires an onGeometryChange callback');
    }
    if (
        !virtualCore
        || typeof virtualCore.Virtualizer !== 'function'
        || typeof virtualCore.defaultRangeExtractor !== 'function'
        || typeof virtualCore.observeElementOffset !== 'function'
        || typeof virtualCore.observeElementRect !== 'function'
    ) {
        throw new TypeError('Chat virtualizer requires the pinned virtual-core bundle');
    }
    if (typeof scrollToFn !== 'function') {
        throw new TypeError('Chat virtualizer requires the ChatSurface scroll port');
    }
    if (!Number.isInteger(maxViewportItems) || maxViewportItems < 1) {
        throw new TypeError('Chat virtualizer maxViewportItems must be a positive integer');
    }
    const {
        Virtualizer,
        defaultRangeExtractor,
        observeElementOffset,
        observeElementRect,
    } = virtualCore;

    /** @type {readonly string[]} */
    let keys = Object.freeze([]);
    /** @type {Readonly<{ paddingStart: number; paddingEnd: number; gap: number }>} */
    let metrics = Object.freeze({ paddingStart: 0, paddingEnd: 0, gap: 0 });
    /** @type {'tail' | 'normal' | 'forced'} */
    let mode = 'tail';
    /** @type {number | null} */
    let forcedIndex = null;
    /** @type {Readonly<{ startIndex: number; endIndex: number; overscan: number; count: number }> | null} */
    let latestRange = null;
    let mounted = false;
    /** @type {number | null} */
    let pendingProgrammaticOffset = null;
    /** @type {(() => void) | null} */
    let unmount = null;

    /** @type {import('@tanstack/virtual-core').VirtualizerOptions<HTMLElement, HTMLElement>['scrollToFn']} */
    const trackedScrollToFn = (offset, options, instance) => {
        pendingProgrammaticOffset = offset + (options.adjustments ?? 0);
        scrollToFn(offset, options, instance);
    };

    /**
     * @param {{ startIndex: number; endIndex: number; overscan: number; count: number }} range
     * @param {'tail' | 'normal' | 'forced'} rangeMode
     * @param {number | null} rangeForcedIndex
     */
    function viewportIndicesForRange(range, rangeMode, rangeForcedIndex) {
        if (rangeMode === 'tail') {
            return [];
        }
        if (rangeMode === 'forced') {
            if (rangeForcedIndex === null) {
                throw new Error('Chat virtualizer forced mode is missing its target');
            }
            return windowAround(range.count, rangeForcedIndex, maxViewportItems);
        }
        return cappedViewportRange(range, maxViewportItems, defaultRangeExtractor);
    }

    /**
     * @param {readonly string[]} optionKeys
     * @returns {import('@tanstack/virtual-core').VirtualizerOptions<HTMLElement, HTMLElement>}
     */
    function createOptions(optionKeys) {
        const capturedKeys = optionKeys;
        const capturedMode = mode;
        const capturedForcedIndex = forcedIndex;
        /** @param {{ startIndex: number; endIndex: number; overscan: number; count: number }} range */
        const rangeExtractor = range => {
            latestRange = Object.freeze({ ...range });
            const viewport = viewportIndicesForRange(range, capturedMode, capturedForcedIndex);
            if (range.count === 0) {
                return viewport;
            }
            const tail = range.count - 1;
            return viewport.includes(tail) ? viewport : [...viewport, tail];
        };
        return {
            count: capturedKeys.length,
            getScrollElement: () => root,
            estimateSize: () => CHAT_VIRTUAL_ESTIMATE_PX,
            getItemKey: /** @param {number} index */ index => {
                const key = capturedKeys[index];
                if (key === undefined) {
                    throw new Error(`Chat virtualizer cannot resolve key ${index}`);
                }
                return key;
            },
            rangeExtractor,
            overscan: CHAT_VIRTUAL_OVERSCAN,
            paddingStart: metrics.paddingStart,
            paddingEnd: metrics.paddingEnd,
            gap: metrics.gap,
            scrollMargin: 0,
            indexAttribute: 'data-tt-virtual-index',
            anchorTo: 'end',
            followOnAppend: 'auto',
            scrollEndThreshold: CHAT_VIRTUAL_SCROLL_END_THRESHOLD_PX,
            isScrollingResetDelay: CHAT_VIRTUAL_SCROLL_RESET_DELAY_MS,
            useScrollendEvent: true,
            useAnimationFrameWithResizeObserver: false,
            observeElementRect,
            observeElementOffset,
            scrollToFn: trackedScrollToFn,
            measureElement: measureMessage,
            onChange(/** @type {any} */ _instance, /** @type {boolean} */ sync) {
                const matchedProgrammaticWrite = sync
                    && pendingProgrammaticOffset !== null
                    && Math.abs(root.scrollTop - pendingProgrammaticOffset) < 1.5;
                if (sync) {
                    pendingProgrammaticOffset = null;
                }
                onGeometryChange(Object.freeze({
                    scrolling: _instance.isScrolling,
                    programmatic: Boolean(_instance.scrollState) || matchedProgrammaticWrite,
                }));
            },
        };
    }

    const virtualizer = new Virtualizer(createOptions(keys));

    function applyOptions() {
        latestRange = null;
        virtualizer.setOptions(createOptions(keys));
    }

    function mount() {
        if (mounted) {
            throw new Error('Chat virtualizer is already mounted');
        }
        mounted = true;
        unmount = virtualizer._didMount();
        virtualizer._willUpdate();
    }

    /** @param {readonly string[]} nextKeys */
    function setStructure(nextKeys) {
        if (!Array.isArray(nextKeys) || !Object.isFrozen(nextKeys)) {
            throw new Error('Chat virtualizer requires immutable structure keys');
        }
        if (new Set(nextKeys).size !== nextKeys.length) {
            throw new Error('Chat virtualizer structure contains duplicate keys');
        }
        keys = nextKeys;
        metrics = readLayoutMetrics(root);
        if (forcedIndex !== null && forcedIndex >= keys.length) {
            forcedIndex = null;
            mode = 'tail';
        }
        applyOptions();
    }

    function refreshMetrics() {
        const next = readLayoutMetrics(root);
        if (
            next.paddingStart !== metrics.paddingStart
            || next.paddingEnd !== metrics.paddingEnd
            || next.gap !== metrics.gap
        ) {
            metrics = next;
            applyOptions();
        }
        return metrics;
    }

    function invalidateMeasurements() {
        refreshMetrics();
        virtualizer.measure();
        return metrics;
    }

    /** @param {'tail' | 'normal'} nextMode */
    function setMode(nextMode) {
        if (nextMode !== 'tail' && nextMode !== 'normal') {
            throw new Error(`Unsupported chat virtualizer mode: ${String(nextMode)}`);
        }
        mode = nextMode;
        forcedIndex = null;
        applyOptions();
    }

    /** @param {number} messageId */
    function force(messageId) {
        if (!Number.isInteger(messageId) || messageId < 0 || messageId >= keys.length) {
            throw new RangeError(`Chat virtualizer target ${messageId} is outside the structure`);
        }
        mode = 'forced';
        forcedIndex = messageId;
        applyOptions();
    }

    function geometry() {
        const virtualItems = virtualizer.getVirtualItems();
        const range = latestRange;
        const viewportIndices = range
            ? viewportIndicesForRange(range, mode, forcedIndex)
            : [];
        const projectedIndices = keys.length === 0
            ? []
            : viewportIndices.includes(keys.length - 1)
                ? viewportIndices
                : [...viewportIndices, keys.length - 1];
        const itemsByIndex = new Map(virtualItems.map(item => [item.index, item]));
        /** @param {number[]} indices */
        const requireItems = indices => indices.map(index => {
            const item = itemsByIndex.get(index);
            if (!item) {
                throw new Error(`Chat virtualizer is missing measurement ${index}`);
            }
            return Object.freeze({
                index: item.index,
                key: item.key,
                start: item.start,
                end: item.end,
                size: item.size,
            });
        });
        const visibleMessageIds = range
            ? viewportIndices.filter(index => index >= range.startIndex && index <= range.endIndex)
            : [];
        return Object.freeze({
            count: keys.length,
            mode,
            metrics,
            viewportItems: Object.freeze(requireItems(viewportIndices)),
            projectedItems: Object.freeze(requireItems(projectedIndices)),
            visibleMessageIds: Object.freeze(visibleMessageIds),
            totalSize: virtualizer.getTotalSize(),
            isScrolling: virtualizer.isScrolling,
            atEnd: virtualizer.isAtEnd(CHAT_VIRTUAL_SCROLL_END_THRESHOLD_PX),
            scrollOffset: virtualizer.scrollOffset ?? 0,
        });
    }

    /** @param {Iterable<HTMLElement>} elements */
    function measure(elements) {
        for (const element of elements) {
            if (!(element instanceof HTMLElement) || !element.matches('.mes')) {
                throw new TypeError('Chat virtualizer can only measure message elements');
            }
            virtualizer.measureElement(element);
        }
        virtualizer.measureElement(null);
        virtualizer._willUpdate();
        return geometry();
    }

    function scrollToEnd() {
        virtualizer.scrollToEnd({ behavior: 'auto' });
        virtualizer.scrollOffset = root.scrollTop;
    }

    /** @param {number} messageId @param {'start' | 'center' | 'end'} [align] */
    function scrollToIndex(messageId, align = 'center') {
        if (!Number.isInteger(messageId) || messageId < 0 || messageId >= keys.length) {
            throw new RangeError(`Chat virtualizer target ${messageId} is outside the structure`);
        }
        virtualizer.scrollToIndex(messageId, { align, behavior: 'auto' });
        virtualizer.scrollOffset = root.scrollTop;
    }

    /** @param {number} messageId @param {number} offset */
    function scrollToAnchor(messageId, offset) {
        if (!Number.isFinite(offset) || offset < 0) {
            throw new TypeError('Chat virtualizer anchor offset must be a non-negative finite number');
        }
        const item = virtualizer.getVirtualItems().find(candidate => candidate.index === messageId);
        if (!item) {
            throw new Error(`Chat virtualizer cannot restore unmounted anchor ${messageId}`);
        }
        const boundedOffset = Math.min(offset, Math.max(item.size - 1, 0));
        scrollToIndex(messageId, 'start');
        if (boundedOffset > 0) {
            virtualizer.scrollToOffset(root.scrollTop + boundedOffset, { behavior: 'auto' });
            virtualizer.scrollOffset = root.scrollTop;
        }
    }

    function dispose() {
        if (!mounted) {
            return;
        }
        mounted = false;
        unmount?.();
        unmount = null;
    }

    function reset() {
        if (mounted) {
            throw new Error('Chat virtualizer must be unmounted before reset');
        }
        keys = Object.freeze([]);
        mode = 'tail';
        forcedIndex = null;
        metrics = Object.freeze({ paddingStart: 0, paddingEnd: 0, gap: 0 });
        latestRange = null;
        pendingProgrammaticOffset = null;
        virtualizer.setOptions(createOptions(keys));
        virtualizer.measure();
    }

    return Object.freeze({
        mount,
        setStructure,
        refreshMetrics,
        invalidateMeasurements,
        setMode,
        force,
        geometry,
        measure,
        scrollToEnd,
        scrollToIndex,
        scrollToAnchor,
        isAtEnd: () => virtualizer.isAtEnd(CHAT_VIRTUAL_SCROLL_END_THRESHOLD_PX),
        reset,
        dispose,
    });
}
