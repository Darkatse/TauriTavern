// @ts-check

/**
 * @typedef {import('../../services/embedded-runtime/types.js').EmbeddedRuntimeSlot} EmbeddedRuntimeSlot
 * @typedef {{ status: 'pending' } | { status: 'ready'; blob: Blob } | { status: 'failed'; error: unknown }} SourceCapture
 * @typedef {{ iframe: HTMLIFrameElement; src: string; srcdoc: string | null; capture: SourceCapture | null; ownedUrl: string }} IframeSource
 */

import { dropParkedManagedIframe, isManagedIframeParked, parkManagedIframe, takeParkedManagedIframe } from './managed-iframe-parking-lot.js';

const BUDGET_PLACEHOLDER_CLASS = 'tt-runtime-placeholder';
const GHOST_PLACEHOLDER_CLASS = 'tt-runtime-ghost';

/**
 * Marks an iframe mutation as managed by TauriTavern embedded-runtime so that
 * chat-level self-healing observers can ignore it.
 *
 * @param {HTMLIFrameElement} iframe
 */
function markManagedIframeMutation(iframe) {
    iframe.dataset.ttRuntimeManaged = '1';
    queueMicrotask(() => {
        // Callers mutate childList after this function returns, so observer
        // delivery is queued behind this microtask. Keep the marker until then.
        queueMicrotask(() => {
            delete iframe.dataset.ttRuntimeManaged;
        });
    });
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
 * @param {() => void} options.onSourceSettled
 * @param {number} [options.priority]
 * @param {number} [options.weight]
 * @returns {EmbeddedRuntimeSlot}
 */
export function createManagedIframeSlot({
    id,
    kind,
    host,
    maxSoftParkedIframes,
    softParkTtlMs,
    onSourceSettled,
    priority = 0,
    weight = 10,
}) {
    if (!(host instanceof HTMLElement)) {
        throw new Error(`createManagedIframeSlot(${id}): host must be an HTMLElement`);
    }
    if (!Number.isFinite(Number(maxSoftParkedIframes))) {
        throw new Error(`createManagedIframeSlot(${id}): maxSoftParkedIframes must be a number`);
    }
    if (!Number.isFinite(Number(softParkTtlMs))) {
        throw new Error(`createManagedIframeSlot(${id}): softParkTtlMs must be a number`);
    }
    if (typeof onSourceSettled !== 'function') {
        throw new TypeError(`createManagedIframeSlot(${id}): onSourceSettled must be a function`);
    }

    /** @type {IframeSource | null} */
    let source = null;
    let disposed = false;
    /** @type {number} */
    let lastMeasuredHeight = 0;

    /** @param {IframeSource} reading */
    const captureBlob = async (reading) => {
        try {
            const response = await fetch(reading.src);
            if (!response.ok) {
                throw new Error(`Embedded iframe source(${id}): HTTP ${response.status}`);
            }
            reading.capture = { status: 'ready', blob: await response.blob() };
        } catch (error) {
            reading.capture = { status: 'failed', error };
        }
        // A completed read owns only its source data, never a pending DOM action.
        // Replacement/disposal drops the source; the manager decides what to do now.
        if (source === reading) {
            if (reading.capture?.status === 'failed') {
                console.warn(`Embedded iframe source(${id}) unavailable`, reading.capture.error);
            }
            onSourceSettled();
        }
    };

    const releaseSource = () => {
        if (source?.ownedUrl) URL.revokeObjectURL(source.ownedUrl);
        source = null;
    };

    const ensureSource = () => {
        const iframe = findHostIframe(host);
        if (!iframe) {
            if (source) return source;
            throw new Error(`createManagedIframeSlot(${id}): iframe is missing`);
        }
        const src = (iframe.getAttribute('src') || '').trim();
        const srcdoc = iframe.getAttribute('srcdoc');
        if (source && source.srcdoc === srcdoc && (src === source.src || (source.ownedUrl && src === source.ownedUrl))) {
            source.iframe = iframe;
            return source;
        }
        releaseSource();
        source = {
            iframe, src, srcdoc, ownedUrl: '',
            capture: srcdoc === null && src.startsWith('blob:') ? { status: 'pending' } : null,
        };
        if (source.capture) void captureBlob(source);
        return source;
    };

    const getRecoveryFailure = () => {
        if (source?.capture?.status === 'failed') return source.capture;
        const iframe = source?.iframe;
        if (iframe?.isConnected && !host.contains(iframe) && !isManagedIframeParked(id, iframe)) {
            return { error: `Embedded iframe(${id}) was moved to another host` };
        }
        return null;
    };

    const canHydrate = () => !disposed && host.isConnected && (findHostIframe(host) !== null
        || (source !== null && source.capture?.status !== 'pending' && getRecoveryFailure() === null));

    const removeIframe = () => {
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
    const ensureBudgetPlaceholder = (heightPx, reason) => {
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
    const ensureGhostPlaceholder = (heightPx) => {
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

    const removePlaceholders = () => {
        const budget = findHostBudgetPlaceholder(host);
        if (budget) {
            budget.remove();
        }
        const ghost = findHostGhostPlaceholder(host);
        if (ghost) {
            ghost.remove();
        }
    };

    /** @param {unknown} error */
    const showRecoveryError = (error) => {
        removePlaceholders();
        const placeholder = ensureBudgetPlaceholder(lastMeasuredHeight || 240, 'source-unavailable');
        placeholder.tabIndex = -1;
        placeholder.title = String(error);
        placeholder.style.cursor = 'default';
        const title = placeholder.querySelector('.tt-runtime-placeholder-title');
        if (title) {
            title.textContent = 'Embedded content unavailable';
        }
        const hint = placeholder.querySelector('.tt-runtime-placeholder-hint');
        if (hint) {
            hint.textContent = 'Cannot restore this page locally. Reopen the chat to reload it.';
        }
    };

    /** @param {HTMLIFrameElement} iframe */
    const measureIframeHeight = (iframe) => {
        const height = Math.round(Number(iframe.getBoundingClientRect().height) || 0) || iframe.offsetHeight || 0;
        if (height > 0) lastMeasuredHeight = height;
        return lastMeasuredHeight || 240;
    };

    /** @param {HTMLIFrameElement} iframe */
    const parkIframe = (iframe) => {
        if (maxSoftParkedIframes > 0 && source?.capture === null) {
            parkManagedIframe({ id, iframe, maxIframes: maxSoftParkedIframes, ttlMs: softParkTtlMs });
        }
    };

    const hydrate = () => {
        if (disposed || !host.isConnected) return;
        const existing = findHostIframe(host);
        const current = ensureSource();
        if (existing) {
            dropParkedManagedIframe(id);
            removePlaceholders();
            return;
        }
        const failure = getRecoveryFailure();
        if (failure) {
            showRecoveryError(failure.error);
            return;
        }
        if (current.capture?.status === 'pending') return;

        // refresh() may have observed a replacement before it too was removed.
        // A parked predecessor must not override the latest renderer element.
        if (!isManagedIframeParked(id, current.iframe)) dropParkedManagedIframe(id);
        const iframe = takeParkedManagedIframe(id) ?? current.iframe;
        markManagedIframeMutation(iframe);
        if (current.capture?.status === 'ready') {
            // The owned URL lives with its source, independently of renderer URLs.
            iframe.remove();
            current.ownedUrl ||= URL.createObjectURL(current.capture.blob);
            iframe.src = current.ownedUrl;
        }
        const placeholder = findHostBudgetPlaceholder(host) ?? findHostGhostPlaceholder(host);
        if (placeholder) placeholder.replaceWith(iframe);
        else host.append(iframe);
        removePlaceholders();
    };

    /** @param {string} reason */
    const dehydrate = (reason) => {
        if (disposed) return;
        const iframe = findHostIframe(host);
        const current = ensureSource();
        if (iframe) dropParkedManagedIframe(id);
        if (iframe && current.capture && current.capture.status !== 'ready') {
            // Keep the live page until we have enough source data to restore it.
            return;
        }
        if (reason !== 'budget' && reason !== 'visibility') {
            removePlaceholders();
            removeIframe();
            return;
        }
        if (reason === 'budget') {
            const failure = getRecoveryFailure();
            if (failure) {
                showRecoveryError(failure.error);
                return;
            }
        }

        const height = iframe ? measureIframeHeight(iframe) : lastMeasuredHeight || 240;
        const obsolete = reason === 'budget' ? findHostGhostPlaceholder(host) : findHostBudgetPlaceholder(host);
        obsolete?.remove();
        const placeholder = reason === 'budget'
            ? ensureBudgetPlaceholder(height, reason)
            : ensureGhostPlaceholder(height);
        if (iframe) {
            markManagedIframeMutation(iframe);
            iframe.replaceWith(placeholder);
            parkIframe(iframe);
        }
    };

    // Capture while the renderer's original URL is still available, before the
    // first scheduled budget/visibility decision can detach the iframe.
    ensureSource();

    return {
        id,
        kind,
        element: host,
        priority,
        weight,
        iframeCount: 1,
        isResident: () => !disposed && host.isConnected && findHostIframe(host) !== null,
        canHydrate,
        refresh: () => {
            if (!disposed) ensureSource();
        },
        hydrate,
        dehydrate,
        dispose: () => {
            disposed = true;
            removeIframe();
            dropParkedManagedIframe(id);
            removePlaceholders();
            releaseSource();
        },
    };
}
