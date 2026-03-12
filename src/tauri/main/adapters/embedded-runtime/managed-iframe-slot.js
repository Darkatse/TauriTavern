// @ts-check

/**
 * @typedef {import('../../services/embedded-runtime/types.js').EmbeddedRuntimeSlot} EmbeddedRuntimeSlot
 */

const PLACEHOLDER_CLASS = 'tt-runtime-placeholder';

/**
 * @param {HTMLIFrameElement} iframe
 */
function cloneIframeTemplate(iframe) {
    const clone = /** @type {HTMLIFrameElement} */ (iframe.cloneNode(true));
    clone.removeAttribute('data-tt-runtime-clone');
    return clone;
}

/**
 * @param {HTMLElement} host
 */
function findHostIframe(host) {
    const iframe = host.querySelector('iframe');
    return iframe instanceof HTMLIFrameElement ? iframe : null;
}

/**
 * @param {HTMLElement} host
 */
function findHostPlaceholder(host) {
    const el = host.querySelector(`.${PLACEHOLDER_CLASS}`);
    return el instanceof HTMLElement ? el : null;
}

/**
 * @param {object} options
 * @param {string} options.id
 * @param {string} options.kind
 * @param {HTMLElement} options.host
 * @param {number} [options.priority]
 * @param {number} [options.weight]
 * @param {number} [options.parkDelayMs]
 * @returns {EmbeddedRuntimeSlot}
 */
export function createManagedIframeSlot({ id, kind, host, priority = 0, weight = 10, parkDelayMs = 1500 }) {
    if (!(host instanceof HTMLElement)) {
        throw new Error(`createManagedIframeSlot(${id}): host must be an HTMLElement`);
    }

    /** @type {HTMLIFrameElement | null} */
    let template = null;
    /** @type {number} */
    let lastMeasuredHeight = 0;
    /** @type {number | null} */
    let parkTimer = null;

    const ensureTemplate = () => {
        const iframe = findHostIframe(host);
        if (!iframe) {
            if (template) {
                return;
            }
            throw new Error(`createManagedIframeSlot(${id}): iframe is missing`);
        }
        if (!template || !iframe.dataset.ttRuntimeClone) {
            template = cloneIframeTemplate(iframe);
        }
    };

    const cancelParkTimer = () => {
        if (parkTimer === null) {
            return;
        }
        clearTimeout(parkTimer);
        parkTimer = null;
    };

    const removeIframeNow = () => {
        const iframe = findHostIframe(host);
        if (!iframe) {
            return;
        }
        iframe.remove();
    };

    /**
     * @param {number} heightPx
     * @param {string} reason
     */
    const ensurePlaceholderNow = (heightPx, reason) => {
        const existing = findHostPlaceholder(host);
        if (existing) {
            existing.style.minHeight = `${heightPx}px`;
            existing.dataset.ttRuntimeParkReason = reason;
            return existing;
        }

        const el = document.createElement('div');
        el.className = PLACEHOLDER_CLASS;
        el.tabIndex = 0;
        el.dataset.ttRuntimeParkReason = reason;
        el.style.minHeight = `${heightPx}px`;

        const title = document.createElement('div');
        title.className = 'tt-runtime-placeholder-title';
        title.textContent = 'Embedded content paused';

        const hint = document.createElement('div');
        hint.className = 'tt-runtime-placeholder-hint';
        hint.textContent = 'Tap to load';

        el.append(title, hint);
        host.append(el);
        return el;
    };

    const removePlaceholderNow = () => {
        const placeholder = findHostPlaceholder(host);
        if (placeholder) {
            placeholder.remove();
        }
    };

    /** @param {HTMLIFrameElement} iframe */
    const measureIframeHeight = (iframe) => {
        const rect = iframe.getBoundingClientRect();
        const h = Math.round(Number(rect?.height) || 0) || iframe.offsetHeight || 0;
        if (h > 0) {
            lastMeasuredHeight = h;
            return h;
        }
        if (lastMeasuredHeight > 0) {
            return lastMeasuredHeight;
        }
        return 240;
    };

    /** @param {string} reason */
    const replaceIframeWithPlaceholderNow = (reason) => {
        const iframe = findHostIframe(host);
        if (!iframe) {
            return;
        }

        ensureTemplate();
        const height = measureIframeHeight(iframe);
        const placeholder = ensurePlaceholderNow(height, reason);
        iframe.replaceWith(placeholder);
    };

    const ensureIframeNow = () => {
        const iframe = findHostIframe(host);
        if (iframe) {
            removePlaceholderNow();
            return;
        }
        ensureTemplate();
        const next = cloneIframeTemplate(/** @type {HTMLIFrameElement} */ (template));
        next.dataset.ttRuntimeClone = '1';

        const placeholder = findHostPlaceholder(host);
        if (placeholder) {
            placeholder.replaceWith(next);
        } else {
            host.append(next);
        }
        removePlaceholderNow();
    };

    const schedulePark = () => {
        if (parkDelayMs <= 0) {
            ensureTemplate();
            removeIframeNow();
            return;
        }

        if (parkTimer !== null) {
            return;
        }

        ensureTemplate();
        parkTimer = setTimeout(() => {
            parkTimer = null;
            removeIframeNow();
        }, parkDelayMs);
    };

    return {
        id,
        kind,
        element: host,
        priority,
        weight,
        iframeCount: 1,
        hydrate: () => {
            cancelParkTimer();
            ensureIframeNow();
        },
        dehydrate: (reason) => {
            if (reason === 'visibility') {
                schedulePark();
                return;
            }
            cancelParkTimer();
            if (reason === 'budget') {
                replaceIframeWithPlaceholderNow(reason);
                return;
            }
            ensureTemplate();
            removePlaceholderNow();
            removeIframeNow();
        },
        dispose: () => {
            cancelParkTimer();
            removePlaceholderNow();
            removeIframeNow();
        },
    };
}
