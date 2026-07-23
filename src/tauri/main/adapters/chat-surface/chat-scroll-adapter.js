// @ts-check

/**
 * The only native `#chat` scroll writer for both upstream calls and the bounded
 * virtualizer. Geometry policy stays outside this adapter.
 *
 * @param {HTMLElement} root
 * @param {{ animateTop?: (top: number, duration: number) => void }} [options]
 */
export function createChatScrollAdapter(root, { animateTop } = {}) {
    if (!(root instanceof HTMLElement)) {
        throw new TypeError('ChatSurface scroll root must be an HTMLElement');
    }

    return Object.freeze({
        top: () => root.scrollTop,
        height: () => root.scrollHeight,
        /** @param {number} top */
        setTop(top) {
            if (!Number.isFinite(top)) {
                throw new TypeError('ChatSurface scroll top must be finite');
            }
            root.scrollTop = top;
        },
        /** @param {number} offset */
        offsetTop(offset) {
            if (!Number.isFinite(offset)) {
                throw new TypeError('ChatSurface scroll offset must be finite');
            }
            root.scrollTop += offset;
        },
        /** @param {ScrollToOptions} options */
        scrollTo(options) {
            root.scrollTo(options);
        },
        /** @param {number} offset @param {{ adjustments?: number; behavior?: ScrollBehavior }} options */
        virtualScrollTo(offset, { adjustments = 0, behavior } = {}) {
            if (!Number.isFinite(offset) || !Number.isFinite(adjustments)) {
                throw new TypeError('ChatSurface virtual scroll requires finite offsets');
            }
            const top = offset + adjustments;
            root.scrollTo(behavior === undefined ? { top } : { top, behavior });
        },
        /** @param {number} top @param {number} duration */
        animateTop(top, duration) {
            if (!Number.isFinite(top) || !Number.isFinite(duration) || duration < 0) {
                throw new TypeError('ChatSurface animated scroll requires finite top and duration');
            }
            if (animateTop) {
                animateTop(top, duration);
                return;
            }
            root.scrollTo({ top, behavior: 'smooth' });
        },
    });
}
