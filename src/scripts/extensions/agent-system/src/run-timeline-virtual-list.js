export const RUN_TIMELINE_ROW_HEIGHT_PX = 58;
export const RUN_TIMELINE_OVERSCAN_ROWS = 8;

export function timelineItemRowSpan(item) {
    if (item?.rowSpan == null) {
        return 1;
    }
    return positiveInteger(item.rowSpan, 'rowSpan');
}

export function timelineItemHeightPx(item, rowHeight = RUN_TIMELINE_ROW_HEIGHT_PX) {
    return timelineItemRowSpan(item) * positiveInteger(rowHeight, 'rowHeight');
}

export function virtualizeTimelineItems(items, scrollTop, viewportHeight, options = {}) {
    const rowHeight = positiveInteger(
        options.rowHeight ?? RUN_TIMELINE_ROW_HEIGHT_PX,
        'rowHeight',
    );
    const overscan = nonNegativeInteger(
        options.overscan ?? RUN_TIMELINE_OVERSCAN_ROWS,
        'overscan',
    );
    const rows = Array.isArray(items) ? items : [];
    const total = rows.length;
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
    const offsets = timelineItemOffsets(rows, rowHeight);
    const totalHeight = offsets[total];

    const top = Math.max(0, finiteNumber(scrollTop, 'scrollTop'));
    const viewport = Math.max(rowHeight, finiteNumber(viewportHeight, 'viewportHeight'));
    const visibleCount = Math.ceil(viewport / rowHeight) + overscan * 2;
    const firstVisible = itemIndexAtOffset(offsets, top);
    const maxStart = Math.max(0, total - visibleCount);
    const start = Math.min(maxStart, Math.max(0, firstVisible - overscan));
    const end = Math.min(total, start + visibleCount);

    return {
        items: rows.slice(start, end),
        start,
        end,
        topPadding: offsets[start],
        bottomPadding: totalHeight - offsets[end],
        totalHeight,
    };
}

function timelineItemOffsets(items, rowHeight) {
    const offsets = [0];
    for (const item of items) {
        offsets.push(offsets[offsets.length - 1] + timelineItemHeightPx(item, rowHeight));
    }
    return offsets;
}

function itemIndexAtOffset(offsets, offset) {
    const total = offsets.length - 1;
    if (offset <= 0) {
        return 0;
    }
    if (offset >= offsets[total]) {
        return total - 1;
    }

    let low = 0;
    let high = total - 1;
    while (low <= high) {
        const mid = Math.floor((low + high) / 2);
        if (offsets[mid + 1] <= offset) {
            low = mid + 1;
        } else if (offsets[mid] > offset) {
            high = mid - 1;
        } else {
            return mid;
        }
    }
    return Math.min(low, total - 1);
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
