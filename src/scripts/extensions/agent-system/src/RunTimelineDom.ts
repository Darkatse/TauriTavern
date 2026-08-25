import type { TimelineViewport } from './RunTimelineContract';

export type TimelineScrollAnchor = { scrollHeight: number; scrollTop: number };

export function readTimelineViewport(scroller: HTMLElement): TimelineViewport {
    return {
        scrollTop: scroller.scrollTop,
        viewportHeight: Math.max(1, scroller.clientHeight),
        nearBottom: scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop < 18,
    };
}

export function captureTimelineScrollAnchor(scroller: HTMLElement | null): TimelineScrollAnchor | null {
    return scroller ? { scrollHeight: scroller.scrollHeight, scrollTop: scroller.scrollTop } : null;
}

export function restoreTimelineScrollAnchor(
    scroller: HTMLElement | null,
    anchor: TimelineScrollAnchor | null,
    onViewport: (viewport: TimelineViewport) => void,
): void {
    if (!scroller || !anchor) return;
    scroller.scrollTop = anchor.scrollTop + Math.max(0, scroller.scrollHeight - anchor.scrollHeight);
    onViewport(readTimelineViewport(scroller));
}

export function scrollTimelineToBottom(
    scroller: HTMLElement | null,
    onViewport: (viewport: TimelineViewport) => void,
): void {
    if (!scroller) return;
    scroller.scrollTop = scroller.scrollHeight;
    onViewport(readTimelineViewport(scroller));
}
