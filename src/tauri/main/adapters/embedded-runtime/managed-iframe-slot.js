// @ts-check

/**
 * @typedef {import('../../services/embedded-runtime/types.js').EmbeddedRuntimeSlot} EmbeddedRuntimeSlot
 */

import { parkManagedIframe, takeParkedManagedIframe } from './managed-iframe-parking-lot.js';

const BUDGET_PLACEHOLDER_CLASS = 'tt-runtime-placeholder';
const GHOST_PLACEHOLDER_CLASS = 'tt-runtime-ghost';

/**
 * Marks an iframe mutation as managed by TauriTavern embedded-runtime so that
 * chat-level observers can ignore it (ER-3.2 self-heal).
 *
 * @param {HTMLIFrameElement} iframe
 */
function markManagedIframeMutation(iframe) {
    iframe.dataset.ttRuntimeManaged = '1';
    queueMicrotask(() => {
        delete iframe.dataset.ttRuntimeManaged;
    });
}

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
function findHostBudgetPlaceholder(host) {
    const el = host.querySelector(`.${BUDGET_PLACEHOLDER_CLASS}`);
    return el instanceof HTMLElement ? el : null;
}

/**
 * @param {HTMLElement} host
 */
function findHostGhostPlaceholder(host) {
    const el = host.querySelector(`.${GHOST_PLACEHOLDER_CLASS}`);
    return el instanceof HTMLElement ? el : null;
}

/**
 * @param {object} options
 * @param {string} options.id
 * @param {string} options.kind
 * @param {HTMLElement} options.host
 * @param {number} options.maxSoftParkedIframes
 * @param {number} options.softParkTtlMs
 * @param {number} [options.priority]
 * @param {number} [options.weight]
 * @returns {EmbeddedRuntimeSlot}
 */
export function createManagedIframeSlot({ id, kind, host, maxSoftParkedIframes, softParkTtlMs, priority = 0, weight = 10 }) {
    if (!(host instanceof HTMLElement)) {
        throw new Error(`createManagedIframeSlot(${id}): host must be an HTMLElement`);
    }
    if (!Number.isFinite(Number(maxSoftParkedIframes))) {
        throw new Error(`createManagedIframeSlot(${id}): maxSoftParkedIframes must be a number`);
    }
    if (!Number.isFinite(Number(softParkTtlMs))) {
        throw new Error(`createManagedIframeSlot(${id}): softParkTtlMs must be a number`);
    }

    /** @type {HTMLIFrameElement | null} */
    let template = null;
    /** @type {number} */
    let lastMeasuredHeight = 0;

    const ensureTemplate = () => {
        if (template) {
            return;
        }
        const iframe = findHostIframe(host);
        if (!(iframe instanceof HTMLIFrameElement)) {
            throw new Error(`createManagedIframeSlot(${id}): iframe is missing`);
        }
        template = cloneIframeTemplate(iframe);
    };

    const removeIframeNow = () => {
        const iframe = findHostIframe(host);
        if (!iframe) {
            return;
        }
        markManagedIframeMutation(iframe);
        iframe.remove();
    };

    /**
     * @param {number} heightPx
     * @param {string} reason
     */
    const ensureBudgetPlaceholderNow = (heightPx, reason) => {
        const existing = findHostBudgetPlaceholder(host);
        if (existing) {
            existing.style.minHeight = `${heightPx}px`;
            existing.dataset.ttRuntimeParkReason = reason;
            return existing;
        }

        const el = document.createElement('div');
        el.className = BUDGET_PLACEHOLDER_CLASS;
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

    /**
     * @param {number} heightPx
     */
    const ensureGhostPlaceholderNow = (heightPx) => {
        const existing = findHostGhostPlaceholder(host);
        if (existing) {
            existing.style.minHeight = `${heightPx}px`;
            return existing;
        }

        const el = document.createElement('div');
        el.className = GHOST_PLACEHOLDER_CLASS;
        el.setAttribute('aria-hidden', 'true');
        el.style.minHeight = `${heightPx}px`;
        host.append(el);
        return el;
    };

    const removePlaceholdersNow = () => {
        const budget = findHostBudgetPlaceholder(host);
        if (budget) {
            budget.remove();
        }
        const ghost = findHostGhostPlaceholder(host);
        if (ghost) {
            ghost.remove();
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

    const replaceIframeWithGhostPlaceholderNow = () => {
        const iframe = findHostIframe(host);
        if (!iframe) {
            return;
        }
        ensureTemplate();
        const height = measureIframeHeight(iframe);
        const ghost = ensureGhostPlaceholderNow(height);
        markManagedIframeMutation(iframe);
        iframe.replaceWith(ghost);
    };

    /**
     * @param {HTMLIFrameElement} iframe
     */
    const softParkIframe = (iframe) => {
        if (!(maxSoftParkedIframes > 0)) {
            markManagedIframeMutation(iframe);
            iframe.remove();
            return;
        }
        markManagedIframeMutation(iframe);
        parkManagedIframe({
            id,
            iframe,
            maxIframes: maxSoftParkedIframes,
            ttlMs: softParkTtlMs,
        });
    };

    /**
     * Ensures the host has a live iframe instance, preferring a parked instance
     * for the same id to avoid reload/flicker.
     */
    const ensureIframeNow = () => {
        const parked = takeParkedManagedIframe(id);
        if (parked) {
            const existing = findHostIframe(host);
            if (existing && existing !== parked) {
                markManagedIframeMutation(existing);
                existing.remove();
            }

            const budgetPlaceholder = findHostBudgetPlaceholder(host);
            if (budgetPlaceholder) {
                budgetPlaceholder.replaceWith(parked);
            } else {
                const ghostPlaceholder = findHostGhostPlaceholder(host);
                if (ghostPlaceholder) {
                    ghostPlaceholder.replaceWith(parked);
                } else {
                    host.append(parked);
                }
            }
            removePlaceholdersNow();
            return;
        }

        const iframe = findHostIframe(host);
        if (iframe) {
            ensureTemplate();
            removePlaceholdersNow();
            return;
        }

        ensureTemplate();
        const next = cloneIframeTemplate(/** @type {HTMLIFrameElement} */ (template));
        next.dataset.ttRuntimeClone = '1';

        const budgetPlaceholder = findHostBudgetPlaceholder(host);
        if (budgetPlaceholder) {
            budgetPlaceholder.replaceWith(next);
        } else {
            const ghostPlaceholder = findHostGhostPlaceholder(host);
            if (ghostPlaceholder) {
                ghostPlaceholder.replaceWith(next);
            } else {
                host.append(next);
            }
        }
        removePlaceholdersNow();
    };

    return {
        id,
        kind,
        element: host,
        priority,
        weight,
        iframeCount: 1,
        hydrate: () => {
            ensureIframeNow();
        },
        dehydrate: (reason) => {
        if (reason === 'budget') {
            const iframe = findHostIframe(host);
            if (iframe) {
                ensureTemplate();
                const height = measureIframeHeight(iframe);
                const placeholder = ensureBudgetPlaceholderNow(height, reason);
                markManagedIframeMutation(iframe);
                iframe.replaceWith(placeholder);
                softParkIframe(iframe);
            }
            return;
        }
            if (reason === 'visibility') {
                const iframe = findHostIframe(host);
                if (iframe) {
                    replaceIframeWithGhostPlaceholderNow();
                    softParkIframe(iframe);
                } else {
                    const height = lastMeasuredHeight > 0 ? lastMeasuredHeight : 240;
                    ensureGhostPlaceholderNow(height);
                }
                return;
            }
            ensureTemplate();
            removePlaceholdersNow();
            removeIframeNow();
        },
        dispose: () => {
            const iframe = findHostIframe(host);
            if (iframe) {
                softParkIframe(iframe);
            }
            removePlaceholdersNow();
        },
    };
}
