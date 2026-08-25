import type { TimelineResizeBounds } from './RunTimelineContract';

const RUN_TIMELINE_HEIGHT_MIN_PX = 132;
export const RUN_TIMELINE_KEYBOARD_STEP_PX = 28;
export const RUN_TIMELINE_PAGE_STEP_PX = 96;

const TOP_EDGE_GAP_PX = 12;

export function normalizeRunTimelineHeightPx(value: number | null | undefined): number | null {
    if (value == null) return null;
    if (!Number.isFinite(value)) {
        throw new Error('Agent run timeline height must be a finite number or null.');
    }
    return Math.round(value);
}

export function clampRunTimelineHeightPx(value: number, bounds: TimelineResizeBounds): number {
    if (!Number.isFinite(value)) throw new Error('Agent run timeline height must be a finite number.');
    const { min, max } = bounds;
    if (!Number.isFinite(min) || !Number.isFinite(max) || max < min) {
        throw new Error('Agent run timeline resize bounds are invalid.');
    }
    return Math.round(Math.min(Math.max(value, min), max));
}

export function runTimelineHeightBounds(input: {
    panelBottom: number;
    topBoundary: number;
    chromeHeight: number;
}): TimelineResizeBounds {
    const { panelBottom, topBoundary, chromeHeight } = input;
    if (![panelBottom, topBoundary, chromeHeight].every(Number.isFinite)) {
        throw new Error('Agent run timeline resize geometry is invalid.');
    }
    const max = Math.floor(panelBottom - topBoundary - chromeHeight - TOP_EDGE_GAP_PX);
    return { min: RUN_TIMELINE_HEIGHT_MIN_PX, max: Math.max(RUN_TIMELINE_HEIGHT_MIN_PX, max) };
}

export function heightFromTopEdgeDrag(input: {
    startHeight: number;
    startY: number;
    currentY: number;
    bounds: TimelineResizeBounds;
}): number {
    return clampRunTimelineHeightPx(input.startHeight + input.startY - input.currentY, input.bounds);
}
