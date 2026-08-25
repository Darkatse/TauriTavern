export const RUN_TIMELINE_ROW_HEIGHT_PX = 58;
export const RUN_TIMELINE_OVERSCAN_ROWS = 8;

export type TimelineVirtualItem = { rowSpan?: number };

export type TimelineVirtualResult<T> = {
    items: T[];
    topPadding: number;
    bottomPadding: number;
};

export function timelineItemRowSpan(item: TimelineVirtualItem): number {
    return item.rowSpan == null ? 1 : positiveInteger(item.rowSpan, 'rowSpan');
}

export function timelineItemHeightPx(item: TimelineVirtualItem): number {
    return timelineItemRowSpan(item) * RUN_TIMELINE_ROW_HEIGHT_PX;
}

export function virtualizeTimelineItems<T extends TimelineVirtualItem>(
    items: readonly T[],
    scrollTop: number,
    viewportHeight: number,
): TimelineVirtualResult<T> {
    const rows = items;
    const total = rows.length;
    if (total === 0) {
        return { items: [], topPadding: 0, bottomPadding: 0 };
    }
    const offsets = timelineItemOffsets(rows);
    const totalHeight = offsetAt(offsets, total);
    const top = Math.max(0, finiteNumber(scrollTop, 'scrollTop'));
    const viewport = Math.max(RUN_TIMELINE_ROW_HEIGHT_PX, finiteNumber(viewportHeight, 'viewportHeight'));
    const visibleCount = Math.ceil(viewport / RUN_TIMELINE_ROW_HEIGHT_PX) + RUN_TIMELINE_OVERSCAN_ROWS * 2;
    const firstVisible = itemIndexAtOffset(offsets, top);
    const maxStart = Math.max(0, total - visibleCount);
    const start = Math.min(maxStart, Math.max(0, firstVisible - RUN_TIMELINE_OVERSCAN_ROWS));
    const end = Math.min(total, start + visibleCount);
    return {
        items: rows.slice(start, end),
        topPadding: offsetAt(offsets, start),
        bottomPadding: totalHeight - offsetAt(offsets, end),
    };
}

function timelineItemOffsets(items: readonly TimelineVirtualItem[]): number[] {
    const offsets = [0];
    for (const item of items) offsets.push(offsetAt(offsets, offsets.length - 1) + timelineItemHeightPx(item));
    return offsets;
}

function itemIndexAtOffset(offsets: readonly number[], offset: number): number {
    const total = offsets.length - 1;
    if (offset <= 0) return 0;
    if (offset >= offsetAt(offsets, total)) return total - 1;
    let low = 0;
    let high = total - 1;
    while (low <= high) {
        const mid = Math.floor((low + high) / 2);
        if (offsetAt(offsets, mid + 1) <= offset) low = mid + 1;
        else if (offsetAt(offsets, mid) > offset) high = mid - 1;
        else return mid;
    }
    return Math.min(low, total - 1);
}

function offsetAt(offsets: readonly number[], index: number): number {
    const value = offsets[index];
    if (value == null) throw new Error('Agent run timeline virtual offset is unavailable.');
    return value;
}

function positiveInteger(value: unknown, name: string): number {
    const number = Number(value);
    if (!Number.isInteger(number) || number <= 0) {
        throw new Error(`Agent run timeline ${name} must be a positive integer.`);
    }
    return number;
}

function finiteNumber(value: unknown, name: string): number {
    const number = Number(value);
    if (!Number.isFinite(number)) throw new Error(`Agent run timeline ${name} must be finite.`);
    return number;
}
