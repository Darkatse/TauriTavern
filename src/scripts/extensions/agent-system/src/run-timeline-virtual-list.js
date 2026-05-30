export const RUN_TIMELINE_ROW_HEIGHT_PX = 58;
export const RUN_TIMELINE_OVERSCAN_ROWS = 8;

export function virtualizeTimelineItems(items, scrollTop, viewportHeight, options = {}) {
    const rowHeight = positiveInteger(
        options.rowHeight ?? RUN_TIMELINE_ROW_HEIGHT_PX,
        'rowHeight',
    );
    const overscan = nonNegativeInteger(
        options.overscan ?? RUN_TIMELINE_OVERSCAN_ROWS,
        'overscan',
    );
    const total = Array.isArray(items) ? items.length : 0;
    if (total === 0) {
        return {
            items: [],
            start: 0,
            end: 0,
            topPadding: 0,
            bottomPadding: 0,
            totalHeight: 0,
        };
    }

    const top = Math.max(0, finiteNumber(scrollTop, 'scrollTop'));
    const viewport = Math.max(rowHeight, finiteNumber(viewportHeight, 'viewportHeight'));
    const visibleCount = Math.ceil(viewport / rowHeight) + overscan * 2;
    const maxStart = Math.max(0, total - visibleCount);
    const start = Math.min(maxStart, Math.max(0, Math.floor(top / rowHeight) - overscan));
    const end = Math.min(total, start + visibleCount);

    return {
        items: items.slice(start, end),
        start,
        end,
        topPadding: start * rowHeight,
        bottomPadding: (total - end) * rowHeight,
        totalHeight: total * rowHeight,
    };
}

function positiveInteger(value, name) {
    const number = Number(value);
    if (!Number.isInteger(number) || number <= 0) {
        throw new Error(`Agent run timeline ${name} must be a positive integer.`);
    }
    return number;
}

function nonNegativeInteger(value, name) {
    const number = Number(value);
    if (!Number.isInteger(number) || number < 0) {
        throw new Error(`Agent run timeline ${name} must be a non-negative integer.`);
    }
    return number;
}

function finiteNumber(value, name) {
    const number = Number(value);
    if (!Number.isFinite(number)) {
        throw new Error(`Agent run timeline ${name} must be finite.`);
    }
    return number;
}
