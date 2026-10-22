// @ts-check

/** @typedef {{ durationMs?: number; easing?: string; translateYPx?: number }} InlineDrawerMotionOptions */
/** @type {WeakMap<HTMLElement, { animation: Animation; cancel: () => void }>} */
const animationByContentEl = new WeakMap();

/** @param {HTMLElement} contentEl */
export function cancelInlineDrawerMotion(contentEl) {
    animationByContentEl.get(contentEl)?.cancel();
}

/**
 * Renders an already committed drawer state. Cancellation settles false and never commits display.
 * @param {HTMLElement} contentEl
 * @param {boolean} open
 * @param {InlineDrawerMotionOptions} [options]
 * @returns {Promise<boolean>}
 */
export function setInlineDrawerContentOpen(contentEl, open, options = {}) {
    cancelInlineDrawerMotion(contentEl);
    const durationMs = Math.max(0, Number(options.durationMs ?? 160));
    const easing = String(options.easing ?? 'cubic-bezier(0.2, 0, 0, 1)');
    const translateYPx = Math.max(0, Number(options.translateYPx ?? 6));

    if (durationMs === 0 || typeof contentEl.animate !== 'function') {
        contentEl.style.display = open ? 'block' : 'none';
        return Promise.resolve(true);
    }

    if (open) contentEl.style.display = 'block';
    contentEl.style.willChange = 'transform, opacity';
    const from = open
        ? { opacity: 0, transform: `translateY(-${translateYPx}px)` }
        : { opacity: 1, transform: 'translateY(0)' };
    const to = open
        ? { opacity: 1, transform: 'translateY(0)' }
        : { opacity: 0, transform: `translateY(-${translateYPx}px)` };
    const animation = contentEl.animate([from, to], { duration: durationMs, easing, fill: 'forwards' });

    return new Promise(resolve => {
        /** @param {boolean} finished */
        const settle = finished => {
            if (animationByContentEl.get(contentEl)?.animation !== animation) return;
            animationByContentEl.delete(contentEl);
            animation.onfinish = null;
            animation.oncancel = null;
            animation.cancel();
            contentEl.style.willChange = '';
            if (finished) contentEl.style.display = open ? 'block' : 'none';
            resolve(finished);
        };
        animationByContentEl.set(contentEl, { animation, cancel: () => settle(false) });
        animation.onfinish = () => settle(true);
        animation.oncancel = () => settle(false);
    });
}
