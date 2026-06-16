export const RUN_TIMELINE_VIEW_GESTURE_ACTION_DETAILS = 'details';
export const RUN_TIMELINE_VIEW_GESTURE_ACTION_TIMELINE = 'timeline';
export const RUN_TIMELINE_VIEW_GESTURE_MIN_DISTANCE_PX = 64;
export const RUN_TIMELINE_VIEW_GESTURE_AXIS_RATIO = 1.5;

const TOUCH_POINTER_TYPE = 'touch';
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

function isPrimaryTouchPointer(event) {
    return event?.isPrimary === true && event.pointerType === TOUCH_POINTER_TYPE;
}

function isGestureStartTarget(target) {
    const element = typeof target?.closest === 'function' ? target : target?.parentElement;
    return Boolean(element && typeof element.closest === 'function' && !element.closest(EXCLUDED_TARGET_SELECTOR));
}

function gestureDelta(gesture, event) {
    return {
        dx: event.clientX - gesture.startX,
        dy: event.clientY - gesture.startY,
    };
}

function hasHorizontalIntent(dx, dy) {
    return Math.abs(dx) >= RUN_TIMELINE_VIEW_GESTURE_MIN_DISTANCE_PX
        && Math.abs(dx) >= Math.abs(dy) * RUN_TIMELINE_VIEW_GESTURE_AXIS_RATIO;
}

export function canStartRunTimelineViewGesture({
    event,
    target,
    collapsed,
    resizing,
    detailsOpen,
    selectedHasDetails,
} = {}) {
    if (!isPrimaryTouchPointer(event) || collapsed || resizing || !isGestureStartTarget(target)) {
        return false;
    }
    return Boolean(detailsOpen || selectedHasDetails);
}

export function createRunTimelineViewGesture(event, detailsOpen) {
    return {
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        detailsOpen: Boolean(detailsOpen),
    };
}

export function shouldCancelRunTimelineViewGesture(gesture, event) {
    if (!gesture || event?.pointerId !== gesture.pointerId) {
        return false;
    }

    const { dx, dy } = gestureDelta(gesture, event);
    const absX = Math.abs(dx);
    const absY = Math.abs(dy);
    return absY >= VERTICAL_CANCEL_DISTANCE_PX && absY > absX * VERTICAL_CANCEL_AXIS_RATIO;
}

export function resolveRunTimelineViewGesture(gesture, event, {
    detailsOpen,
    selectedHasDetails,
} = {}) {
    if (!gesture || event?.pointerId !== gesture.pointerId || Boolean(detailsOpen) !== gesture.detailsOpen) {
        return null;
    }

    const { dx, dy } = gestureDelta(gesture, event);
    if (!hasHorizontalIntent(dx, dy)) {
        return null;
    }
    if (gesture.detailsOpen) {
        return dx > 0 ? RUN_TIMELINE_VIEW_GESTURE_ACTION_TIMELINE : null;
    }
    return dx < 0 && selectedHasDetails ? RUN_TIMELINE_VIEW_GESTURE_ACTION_DETAILS : null;
}
