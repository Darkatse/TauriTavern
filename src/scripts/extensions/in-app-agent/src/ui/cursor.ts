import type { InteractionPoint } from './interact';
import { planCursorMove, press, ripple } from './cursor-motion';
import type { CursorPose, CursorSample } from './cursor-motion';

const PRESS = press.frames.map(scale => ({ transform: `scale(${scale})` }));
const RIPPLE = ripple.frames.map(({ scale, opacity }) => ({ transform: `scale(${scale})`, opacity }));

type Cursor = ReturnType<typeof createCursor>;

/** A presentation-only cursor. Tool execution never waits for its animation. */
export function createInteractionCursor(getModalLayer: () => HTMLElement) {
    let cursor: Cursor | null = null;
    let frame = 0;
    let animations: Animation[] = [];

    function render(point: InteractionPoint) {
        const from = cursor?.element.isConnected ? currentPose(cursor) : null;
        const { element, heading, glow, arrow, pulse } = cursor ??= createCursor();

        // Modern WebViews render above dialogs. Older ones reuse SillyTavern's modal layer.
        const hasPopover = typeof element.showPopover === 'function';
        const parent = hasPopover ? document.body : getModalLayer();
        if (element.parentElement !== parent) parent.append(element);
        if (hasPopover) {
            element.popover = 'manual';
            element.hidePopover();
            element.showPopover();
        }

        stop();
        element.style.transform = 'none';
        const origin = element.getBoundingClientRect();
        // A fallback dialog can be transformed; convert viewport points into its local coordinates.
        const scaleX = origin.width / element.offsetWidth;
        const scaleY = origin.height / element.offsetHeight;
        const translate = (x: number, y: number) => `translate3d(${(x - origin.left) / scaleX}px, ${(y - origin.top) / scaleY}px, 0)`;
        element.style.transform = translate(point.x, point.y);

        if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
        const plan = from && planCursorMove(from, point);
        if (plan) {
            const timing = { duration: plan.duration };
            const frames = (keyframe: (sample: CursorSample) => Keyframe) =>
                plan.samples.map(sample => ({ ...keyframe(sample), offset: sample.t / plan.duration }));
            animations.push(
                element.animate(frames(({ x, y }) => ({ transform: translate(x, y) })), timing),
                heading.animate(frames(({ rotation }) => ({ transform: `rotate(${rotation}rad)` })), timing),
                // The glow trails the motion, not the arrow; undo the arrow's turn.
                glow.animate(frames(({ rotation, glow: { x, y, scale, opacity } }) => {
                    const cos = Math.cos(rotation);
                    const sin = Math.sin(rotation);
                    return { opacity, transform: `translate(${x * cos + y * sin}px, ${y * cos - x * sin}px) scale(${scale})` };
                }), timing),
            );
        } else {
            animations.push(element.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 140, easing: 'cubic-bezier(.22, 1, .36, 1)' }));
        }
        const delay = plan ? plan.arrival : 0;
        animations.push(
            arrow.animate(PRESS, { duration: press.duration, delay }),
            pulse.animate(RIPPLE, { duration: ripple.duration, delay }),
        );
    }

    function move(point: InteractionPoint) {
        // Coalesce same-frame actions; never replay a queue of stale cursor positions.
        cancelAnimationFrame(frame);
        frame = requestAnimationFrame(() => {
            frame = 0;
            render(point);
        });
    }

    function stop() {
        for (const animation of animations) animation.cancel();
        animations = [];
    }

    function clear() {
        cancelAnimationFrame(frame);
        frame = 0;
        stop();
        cursor?.element.remove();
        cursor = null;
    }

    return { move, clear };
}

function createCursor() {
    const part = (className: string) => Object.assign(document.createElement('span'), { className });
    const element = Object.assign(document.createElement('div'), { className: 'ttia-agent-cursor' });
    element.setAttribute('aria-hidden', 'true');
    const heading = part('ttia-cursor-heading');
    const glow = part('ttia-cursor-glow');
    const arrow = part('ttia-cursor-arrow');
    const pulse = part('ttia-cursor-pulse');
    arrow.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M4.6 3.7 19.1 9.2Q20.5 9.75 19 10.4L12.9 12.9 10.4 19Q9.75 20.5 9.2 19.1L3.7 4.6Q3.2 3.2 4.6 3.7Z" />
    </svg>`;
    heading.append(glow, arrow);
    element.append(heading, pulse);
    return { element, heading, glow, arrow, pulse };
}

/** Where the cursor is drawn now, including mid-move. */
function currentPose({ element, heading }: Cursor): CursorPose {
    const { left, top } = element.getBoundingClientRect();
    const { a, b } = new DOMMatrixReadOnly(getComputedStyle(heading).transform);
    return { x: left, y: top, rotation: Math.atan2(b, a) };
}
