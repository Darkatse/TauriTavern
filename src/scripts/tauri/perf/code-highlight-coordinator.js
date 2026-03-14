// @ts-check

import { hljs } from '../../../lib.js';

let singleton = null;

export function getCodeHighlightCoordinator() {
    if (singleton) {
        return singleton;
    }

    singleton = createCodeHighlightCoordinator();
    return singleton;
}

function createCodeHighlightCoordinator() {
    /** @type {Set<HTMLElement>} */
    const queue = new Set();

    /** @type {WeakMap<HTMLElement, (() => void) | null>} */
    const afterHighlightByEl = new WeakMap();

    let scheduled = false;

    /** @type {IntersectionObserver | null} */
    const observer = typeof IntersectionObserver === 'function'
        ? new IntersectionObserver((entries) => {
            for (const entry of entries) {
                if (!entry.isIntersecting) {
                    continue;
                }
                const target = /** @type {HTMLElement} */ (entry.target);
                observer?.unobserve(target);
                enqueue(target);
            }
            schedule();
        }, {
            root: null,
            rootMargin: '300px 0px',
            threshold: 0.01,
        })
        : null;

    function enqueue(codeEl) {
        if (codeEl.dataset.ttHljsState === 'done') {
            return;
        }
        queue.add(codeEl);
        codeEl.dataset.ttHljsState = 'queued';
    }

    function schedule() {
        if (scheduled) {
            return;
        }

        if (queue.size === 0) {
            return;
        }

        scheduled = true;

        if (typeof requestIdleCallback === 'function') {
            requestIdleCallback(run, { timeout: 500 });
            return;
        }

        requestAnimationFrame(() => run(null));
    }

    /**
     * @param {IdleDeadline | null} deadline
     */
    function run(deadline) {
        scheduled = false;
        if (queue.size === 0) {
            return;
        }

        if (!hljs || typeof hljs.highlightElement !== 'function') {
            throw new Error('CodeHighlightCoordinator: hljs.highlightElement is unavailable');
        }

        const start = performance.now();
        const budgetMs = 8;

        for (const codeEl of queue) {
            queue.delete(codeEl);

            if (codeEl.dataset.ttHljsState === 'done') {
                continue;
            }

            hljs.highlightElement(codeEl);
            codeEl.dataset.ttHljsState = 'done';

            const after = afterHighlightByEl.get(codeEl);
            if (after) {
                afterHighlightByEl.delete(codeEl);
                after();
            }

            if (deadline && typeof deadline.timeRemaining === 'function' && deadline.timeRemaining() < 1) {
                break;
            }

            if (performance.now() - start > budgetMs) {
                break;
            }
        }

        schedule();
    }

    /**
     * @param {HTMLElement} codeEl
     * @param {{ afterHighlight?: (() => void) | null }} [options]
     */
    function request(codeEl, options = {}) {
        const state = String(codeEl.dataset.ttHljsState || '');
        if (state === 'done') {
            return;
        }

        if (Object.prototype.hasOwnProperty.call(options, 'afterHighlight')) {
            afterHighlightByEl.set(codeEl, options.afterHighlight ?? null);
        }

        if (observer) {
            if (state !== 'observing') {
                codeEl.dataset.ttHljsState = 'observing';
                observer.observe(codeEl);
            }
            return;
        }

        enqueue(codeEl);
        schedule();
    }

    function reset() {
        queue.clear();
        scheduled = false;
        observer?.disconnect();
    }

    return {
        request,
        reset,
    };
}
