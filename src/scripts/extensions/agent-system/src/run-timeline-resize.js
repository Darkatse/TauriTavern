export const RUN_TIMELINE_HEIGHT_MIN_PX = 132;
export const RUN_TIMELINE_KEYBOARD_STEP_PX = 28;
export const RUN_TIMELINE_PAGE_STEP_PX = 96;

const TOP_EDGE_GAP_PX = 12;

export function normalizeRunTimelineHeightPx(value) {
    if (value == null) {
        return null;
    }
    if (typeof value !== 'number' || !Number.isFinite(value)) {
        throw new Error('Agent run timeline height must be a finite number or null.');
    }
    return Math.round(value);
}

export function clampRunTimelineHeightPx(value, bounds) {
    if (typeof value !== 'number' || !Number.isFinite(value)) {
        throw new Error('Agent run timeline height must be a finite number.');
    }
    const min = Number(bounds?.min);
    const max = Number(bounds?.max);
    if (!Number.isFinite(min) || !Number.isFinite(max) || max < min) {
        throw new Error('Agent run timeline resize bounds are invalid.');
    }
    return Math.round(Math.min(Math.max(value, min), max));
}

export function runTimelineHeightBounds({ panelBottom, topBoundary, chromeHeight }) {
    const bottom = Number(panelBottom);
    const top = Number(topBoundary);
    const chrome = Number(chromeHeight);
    if (!Number.isFinite(bottom) || !Number.isFinite(top) || !Number.isFinite(chrome)) {
        throw new Error('Agent run timeline resize geometry is invalid.');
    }

    const max = Math.floor(bottom - top - chrome - TOP_EDGE_GAP_PX);
    return {
        min: RUN_TIMELINE_HEIGHT_MIN_PX,
        max: Math.max(RUN_TIMELINE_HEIGHT_MIN_PX, max),
    };
}

export function heightFromTopEdgeDrag({ startHeight, startY, currentY, bounds }) {
    return clampRunTimelineHeightPx(startHeight + startY - currentY, bounds);
}
