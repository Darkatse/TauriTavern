import type { InteractionPoint } from './interact';

/** A presentation-only cursor. Tool execution never waits for its animation. */
export function createInteractionCursor(getModalLayer: () => HTMLElement) {
    let element: HTMLDivElement | null = null;
    let frame = 0;
    let movement: Animation | null = null;
    let pulse: Animation | null = null;

    function render(point: InteractionPoint) {
        const previous = element?.isConnected ? element.getBoundingClientRect() : null;
        if (!element) {
            element = document.createElement('div');
            element.className = 'ttia-agent-cursor';
            element.setAttribute('aria-hidden', 'true');
            element.innerHTML = `<span class="ttia-cursor-pulse"></span>
                <svg viewBox="0 0 24 24" aria-hidden="true">
                    <path d="M4.6 3.7 19.1 9.2Q20.5 9.75 19 10.4L12.9 12.9 10.4 19Q9.75 20.5 9.2 19.1L3.7 4.6Q3.2 3.2 4.6 3.7Z" />
                </svg>`;
        }

        // Modern WebViews render above dialogs. Older ones reuse SillyTavern's modal layer.
        const hasPopover = typeof element.showPopover === 'function';
        const parent = hasPopover ? document.body : getModalLayer();
        if (element.parentElement !== parent) parent.append(element);
        if (hasPopover) {
            element.popover = 'manual';
            element.hidePopover();
            element.showPopover();
        }

        movement?.cancel();
        pulse?.cancel();
        element.style.transform = 'none';
        const origin = element.getBoundingClientRect();
        // A fallback dialog can be transformed; convert viewport points into its local coordinates.
        const scaleX = origin.width / element.offsetWidth;
        const scaleY = origin.height / element.offsetHeight;
        const translate = (x: number, y: number) => `translate3d(${(x - origin.left) / scaleX}px, ${(y - origin.top) / scaleY}px, 0)`;
        const destination = translate(point.x, point.y);
        element.style.transform = destination;

        if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
        const duration = previous ? 240 : 0;
        movement = element.animate(previous
            ? [{ transform: translate(previous.left, previous.top) }, { transform: destination }]
            : [{ opacity: 0 }, { opacity: 1 }],
        { duration: previous ? duration : 140, easing: 'cubic-bezier(.22, 1, .36, 1)' });
        pulse = element.firstElementChild?.animate([
            { opacity: 0, transform: 'scale(.6)' },
            { opacity: .55, transform: 'scale(1)', offset: .2 },
            { opacity: 0, transform: 'scale(1.8)' },
        ], { duration: 440, delay: duration, easing: 'ease-out' }) ?? null;
    }

    function move(point: InteractionPoint) {
        // Coalesce same-frame actions; never replay a queue of stale cursor positions.
        cancelAnimationFrame(frame);
        frame = requestAnimationFrame(() => {
            frame = 0;
            render(point);
        });
    }

    function clear() {
        cancelAnimationFrame(frame);
        frame = 0;
        movement?.cancel();
        pulse?.cancel();
        movement = pulse = null;
        element?.remove();
        element = null;
    }

    return { move, clear };
}
