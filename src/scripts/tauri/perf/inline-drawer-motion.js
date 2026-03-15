// @ts-check

/**
 * @typedef {{ durationMs?: number; easing?: string; translateYPx?: number }} InlineDrawerMotionOptions
 */

/** @type {WeakMap<HTMLElement, Animation>} */
const animationByContentEl = new WeakMap();

/**
 * @param {HTMLElement} contentEl
 */
export function isInlineDrawerContentOpen(contentEl) {
    const state = String(contentEl.dataset.ttInlineDrawerOpen || '');
    if (state === '1') {
        return true;
    }
    if (state === '0') {
        return false;
    }
    return getComputedStyle(contentEl).display !== 'none';
}

/**
 * @param {HTMLElement} contentEl
 * @param {boolean} open
 * @param {InlineDrawerMotionOptions} [options]
 * @returns {Promise<void>}
 */
export function setInlineDrawerContentOpen(contentEl, open, options = {}) {
    contentEl.dataset.ttInlineDrawerOpen = open ? '1' : '0';

    const durationMs = Math.max(0, Number(options.durationMs ?? 160));
    const easing = String(options.easing ?? 'cubic-bezier(0.2, 0, 0, 1)');
    const translateYPx = Math.max(0, Number(options.translateYPx ?? 6));

    const existing = animationByContentEl.get(contentEl);
    if (existing) {
        existing.cancel();
        animationByContentEl.delete(contentEl);
    }

    if (open) {
        contentEl.style.display = 'block';
    }

    if (durationMs === 0) {
        contentEl.style.opacity = '';
        contentEl.style.transform = '';
        contentEl.style.willChange = '';
        contentEl.style.display = open ? 'block' : 'none';
        return Promise.resolve();
    }

    contentEl.style.willChange = 'transform, opacity';

    const from = open
        ? { opacity: 0, transform: `translateY(-${translateYPx}px)` }
        : { opacity: 1, transform: 'translateY(0)' };
    const to = open
        ? { opacity: 1, transform: 'translateY(0)' }
        : { opacity: 0, transform: `translateY(-${translateYPx}px)` };

    const animation = contentEl.animate([from, to], {
        duration: durationMs,
        easing,
        fill: 'forwards',
    });

    animationByContentEl.set(contentEl, animation);

    return new Promise((resolve) => {
        animation.onfinish = () => {
            const current = animationByContentEl.get(contentEl);
            if (current !== animation) {
                resolve();
                return;
            }

            animationByContentEl.delete(contentEl);
            contentEl.style.opacity = '';
            contentEl.style.transform = '';
            contentEl.style.willChange = '';
            contentEl.style.display = open ? 'block' : 'none';
            resolve();
        };

        animation.oncancel = () => {
            const current = animationByContentEl.get(contentEl);
            if (current === animation) {
                animationByContentEl.delete(contentEl);
            }
            contentEl.style.willChange = '';
            resolve();
        };
    });
}
