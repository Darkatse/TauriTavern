// @ts-check

/**
 * @typedef {import('../../services/embedded-runtime/types.js').EmbeddedRuntimeSlot} EmbeddedRuntimeSlot
 */

import { dropParkedManagedIframe, parkManagedIframe, takeParkedManagedIframe } from './managed-iframe-parking-lot.js';

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
export function createManagedIframeSlot({
    id,
    kind,
    host,
    maxSoftParkedIframes,
    softParkTtlMs,
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

    /** @type {HTMLIFrameElement | null} */
    let template = null;
    /** @type {HTMLIFrameElement | null} */
    let sourceIframe = null;
    let sourceUrl = '';
    let ownedBlobUrl = '';
    /** @type {Promise<{ blob: Blob } | { error: unknown }> | null} */
    let sourceBlob = null;
    /** @type {{ blob: Blob } | { error: unknown } | null} */
    let capturedSource = null;
    let revision = 0;
    let disposed = false;
    /** @type {number} */
    let lastMeasuredHeight = 0;

    const ensureTemplate = () => {
        const iframe = findHostIframe(host);
        if (!(iframe instanceof HTMLIFrameElement)) {
            if (template) {
                return;
            }
            throw new Error(`createManagedIframeSlot(${id}): iframe is missing`);
        }
        const src = iframe.getAttribute('src') || '';
        if (template?.srcdoc === iframe.srcdoc
            && template.hasAttribute('srcdoc') === iframe.hasAttribute('srcdoc')
            && ((sourceIframe === iframe && src === sourceUrl) || (ownedBlobUrl && src === ownedBlobUrl))) {
            // An upstream replacement can keep our URL. Transfer the element,
            // not the URL's lifetime; it still uses the same captured source.
            sourceIframe = iframe;
            template = cloneIframeTemplate(iframe);
            return;
        }
        if (ownedBlobUrl) {
            URL.revokeObjectURL(ownedBlobUrl);
            ownedBlobUrl = '';
        }
        template = cloneIframeTemplate(iframe);
        sourceIframe = iframe;
        sourceUrl = src;
        capturedSource = null;
        // Read the renderer-owned URL before detaching can cause it to be revoked.
        // Keep source bytes, not a serialization of the already-running document.
        sourceBlob = !iframe.hasAttribute('srcdoc') && src.startsWith('blob:')
            ? fetch(src).then(response => {
                if (!response.ok) {
                    throw new Error(`Embedded iframe source(${id}): HTTP ${response.status}`);
                }
                return response.blob();
            }).then(blob => ({ blob }), error => ({ error }))
            : null;
        const reading = sourceBlob;
        reading?.then(result => {
            if (!disposed && sourceBlob === reading) {
                capturedSource = result;
            }
        });
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

    /** @param {unknown} error */
    const showRecoveryError = (error) => {
        removePlaceholdersNow();
        const placeholder = ensureBudgetPlaceholderNow(lastMeasuredHeight || 240, 'source-unavailable');
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
        if (!(maxSoftParkedIframes > 0) || sourceBlob) {
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
     * Ensures the host has a live iframe instance, keeping an upstream
     * replacement when present and otherwise reusing a parked instance.
     */
    const ensureIframeNow = async () => {
        const restoreRevision = ++revision;
        if (disposed) {
            return;
        }
        const existing = findHostIframe(host);
        if (existing) {
            // The upstream renderer may have replaced a parked iframe with a
            // fresh one. Keep the fresh instance instead of reviving a stale
            // (and possibly revoked) blob URL from the parking lot.
            dropParkedManagedIframe(id);
            ensureTemplate();
            removePlaceholdersNow();
            await sourceBlob;
            return;
        }

        let next = takeParkedManagedIframe(id);
        if (next && sourceBlob) {
            markManagedIframeMutation(next);
            next.remove();
            next = null;
        }
        if (!next) {
            ensureTemplate();
            if (sourceIframe?.isConnected) {
                // The renderer has moved this element to another owner. Do not
                // steal it (or navigate it by assigning a replacement Blob URL).
                showRecoveryError(`Embedded iframe(${id}) was moved to another host`);
                return;
            }
            if (sourceBlob) {
                const captured = await sourceBlob;
                if (disposed || restoreRevision !== revision || !host.isConnected) {
                    return;
                }
                if (findHostIframe(host)) {
                    ensureTemplate();
                    removePlaceholdersNow();
                    return;
                }
                if (sourceIframe?.isConnected) {
                    showRecoveryError(`Embedded iframe(${id}) was moved to another host`);
                    return;
                }
                if ('error' in captured) {
                    showRecoveryError(captured.error);
                    return;
                }
                if (ownedBlobUrl) {
                    URL.revokeObjectURL(ownedBlobUrl);
                }
                ownedBlobUrl = URL.createObjectURL(captured.blob);
                // Reuse only this renderer's element, preserving its load listener.
                // A fresh URL avoids both stale-blob failures and message-wide updates.
                next = /** @type {HTMLIFrameElement} */ (sourceIframe);
                next.src = ownedBlobUrl;
            } else {
                // srcdoc / ordinary URLs also need the renderer's load listener
                // when their soft-parked browsing context has been evicted.
                next = /** @type {HTMLIFrameElement} */ (sourceIframe);
            }
        }

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
        hydrate: ensureIframeNow,
        dehydrate: function dehydrate(reason) {
            revision += 1;
            if (disposed) {
                return;
            }
            if (findHostIframe(host)) {
                ensureTemplate();
                if (sourceBlob && !capturedSource) {
                    // Never discard a running page before its original source
                    // has been read. A later hydrate/dispose cancels this park.
                    const parkRevision = revision;
                    sourceBlob.then(result => {
                        if ('blob' in result && !disposed && revision === parkRevision && host.isConnected) {
                            dehydrate(reason);
                        }
                    });
                    return;
                }
                if (capturedSource && 'error' in capturedSource) {
                    return;
                }
            }
            if (reason === 'budget') {
                const ghost = findHostGhostPlaceholder(host);
                if (ghost) {
                    ghost.remove();
                }

                const iframe = findHostIframe(host);
                if (iframe) {
                    ensureTemplate();
                    const height = measureIframeHeight(iframe);
                    const placeholder = ensureBudgetPlaceholderNow(height, reason);
                    markManagedIframeMutation(iframe);
                    iframe.replaceWith(placeholder);
                    softParkIframe(iframe);
                    return;
                }

                const height = lastMeasuredHeight > 0 ? lastMeasuredHeight : 240;
                ensureBudgetPlaceholderNow(height, reason);
                return;
            }
            if (reason === 'visibility') {
                const budget = findHostBudgetPlaceholder(host);
                if (budget) {
                    budget.remove();
                }

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
            disposed = true;
            revision += 1;
            removeIframeNow();
            dropParkedManagedIframe(id);
            removePlaceholdersNow();
            if (ownedBlobUrl) {
                URL.revokeObjectURL(ownedBlobUrl);
                ownedBlobUrl = '';
            }
            sourceBlob = null;
            capturedSource = null;
            sourceIframe = null;
            template = null;
        },
    };
}
