export const RUN_TIMELINE_VIEW_GESTURE_ACTION_DETAILS = 'details';
export const RUN_TIMELINE_VIEW_GESTURE_ACTION_TIMELINE = 'timeline';
export const RUN_TIMELINE_VIEW_GESTURE_MIN_DISTANCE_PX = 64;
export const RUN_TIMELINE_VIEW_GESTURE_AXIS_RATIO = 1.5;

const VERTICAL_CANCEL_DISTANCE_PX = 24;
const VERTICAL_CANCEL_AXIS_RATIO = 1.2;
const EXCLUDED_TARGET_SELECTOR = [
    '.ttas-run-header',
    '.ttas-run-resize-handle',
    '.ttas-run-detail-head',
    '.ttas-run-detail-nav',
    '.ttas-run-detail-actions',
    '.ttas-run-detail-block-head',
    '.ttas-run-detail-block pre',
    '.ttas-run-diff',
    '.ttas-subagent-tray',
    'a[href]',
    'input',
    'textarea',
    'select',
    'option',
    '[contenteditable="true"]',
].join(', ');

export type RunTimelineViewGesture = {
    pointerId: number;
    startX: number;
    startY: number;
    detailsOpen: boolean;
};

type PointerCoordinates = {
    pointerId: number;
    clientX: number;
    clientY: number;
};

export function canStartRunTimelineViewGesture(input: {
    event: PointerEvent;
    collapsed: boolean;
    resizing: boolean;
    detailsOpen: boolean;
    selectedHasDetails: boolean;
}): boolean {
    const { event, collapsed, resizing, detailsOpen, selectedHasDetails } = input;
    if (!event.isPrimary || event.pointerType !== 'touch' || collapsed || resizing) return false;
    const element = event.target instanceof Element ? event.target : null;
    if (!element || element.closest(EXCLUDED_TARGET_SELECTOR)) return false;
    return detailsOpen || selectedHasDetails;
}

export function createRunTimelineViewGesture(
    event: PointerCoordinates,
    detailsOpen: boolean,
): RunTimelineViewGesture {
    return {
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        detailsOpen,
    };
}

export function shouldCancelRunTimelineViewGesture(
    gesture: RunTimelineViewGesture | null,
    event: PointerCoordinates,
): boolean {
    if (!gesture || event.pointerId !== gesture.pointerId) return false;
    const { dx, dy } = delta(gesture, event);
    return Math.abs(dy) >= VERTICAL_CANCEL_DISTANCE_PX
        && Math.abs(dy) > Math.abs(dx) * VERTICAL_CANCEL_AXIS_RATIO;
}

export function resolveRunTimelineViewGesture(
    gesture: RunTimelineViewGesture | null,
    event: PointerCoordinates,
    input: { detailsOpen: boolean; selectedHasDetails: boolean },
): typeof RUN_TIMELINE_VIEW_GESTURE_ACTION_DETAILS
    | typeof RUN_TIMELINE_VIEW_GESTURE_ACTION_TIMELINE
    | null {
    if (!gesture || event.pointerId !== gesture.pointerId || input.detailsOpen !== gesture.detailsOpen) return null;
    const { dx, dy } = delta(gesture, event);
    if (Math.abs(dx) < RUN_TIMELINE_VIEW_GESTURE_MIN_DISTANCE_PX
        || Math.abs(dx) < Math.abs(dy) * RUN_TIMELINE_VIEW_GESTURE_AXIS_RATIO) return null;
    if (gesture.detailsOpen) return dx > 0 ? RUN_TIMELINE_VIEW_GESTURE_ACTION_TIMELINE : null;
    return dx < 0 && input.selectedHasDetails ? RUN_TIMELINE_VIEW_GESTURE_ACTION_DETAILS : null;
}

function delta(gesture: RunTimelineViewGesture, event: PointerCoordinates): { dx: number; dy: number } {
    return { dx: event.clientX - gesture.startX, dy: event.clientY - gesture.startY };
}
