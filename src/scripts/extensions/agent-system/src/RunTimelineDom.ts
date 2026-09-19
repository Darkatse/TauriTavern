import type { TimelineResizeBounds, TimelineViewport } from './RunTimelineContract';
import { runTimelineHeightBounds } from './run-timeline-resize';

function layoutTop(element: HTMLElement): number {
    // Layout coordinates exclude the Android composer's temporary IME transform.
    let top = element.offsetTop;
    while (element.offsetParent instanceof HTMLElement) {
        element = element.offsetParent;
        top += element.offsetTop + element.clientTop;
    }
    return top;
}

export function readTimelineHeightBounds(panel: HTMLElement, header: HTMLElement): TimelineResizeBounds {
    const anchor = panel.parentElement;
    if (!anchor) throw new Error('Agent run timeline anchor is unavailable.');
    const topBar = document.getElementById('top-bar');
    return runTimelineHeightBounds({
        panelBottom: layoutTop(anchor),
        topBoundary: topBar ? layoutTop(topBar) + topBar.offsetHeight : 0,
        chromeHeight: header.offsetHeight,
    });
}

export function observeTimelineHeight(
    panel: HTMLElement,
    header: HTMLElement,
    onChange: (height: number) => void,
): () => void {
    const update = () => onChange(readTimelineHeightBounds(panel, header).max);
    update();
    const observer = new ResizeObserver(update);
    // #form_sheld includes the IME spacer; observe the actual input content instead.
    for (const element of [document.documentElement, document.getElementById('send_form'), document.getElementById('top-bar'), header]) {
        if (element) observer.observe(element);
    }
    return () => observer.disconnect();
}

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
