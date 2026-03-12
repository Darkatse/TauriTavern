// @ts-check

/**
 * A tiny global parking lot that keeps iframe browsing contexts alive by
 * moving them into a hidden DOM container instead of destroying them.
 *
 * This is a Phase-1 mechanism:
 * - Keeps scroll-driven runtimes smooth (no reload when coming back).
 * - Enforces a hard cap (mobile) by evicting oldest parked iframes.
 * - Uses TTL to avoid leaking parked instances after DOM rebuilds.
 */

const LOT_ID = 'tt-embedded-runtime-iframe-parking-lot';

/** @type {HTMLDivElement | null} */
let lot = null;

/** @type {Map<string, { iframe: HTMLIFrameElement; parkedAt: number }>} */
const parkedById = new Map();

let lastMaxIframes = 0;
let lastTtlMs = 0;

function nowMs() {
    return globalThis.performance?.now?.() ?? Date.now();
}

function requireLot() {
    if (lot && lot.isConnected) {
        return lot;
    }

    const existing = document.getElementById(LOT_ID);
    if (existing instanceof HTMLDivElement) {
        lot = existing;
        return existing;
    }

    const el = document.createElement('div');
    el.id = LOT_ID;
    el.style.position = 'fixed';
    el.style.left = '0';
    el.style.top = '0';
    el.style.width = '0';
    el.style.height = '0';
    el.style.overflow = 'hidden';
    el.style.pointerEvents = 'none';
    el.style.opacity = '0';
    el.style.zIndex = '-1';
    document.body.append(el);
    lot = el;
    return el;
}

/**
 * @param {number} maxIframes
 * @param {number} ttlMs
 */
function evictIfNeeded(maxIframes, ttlMs) {
    const now = nowMs();
    if (ttlMs > 0) {
        for (const [id, entry] of parkedById.entries()) {
            if (now - entry.parkedAt <= ttlMs) {
                continue;
            }
            entry.iframe.remove();
            parkedById.delete(id);
        }
    }

    if (!(maxIframes > 0) || parkedById.size <= maxIframes) {
        return;
    }

    const victims = [...parkedById.entries()]
        .sort((a, b) => a[1].parkedAt - b[1].parkedAt)
        .slice(0, parkedById.size - maxIframes);

    for (const [id, entry] of victims) {
        entry.iframe.remove();
        parkedById.delete(id);
    }
}

/**
 * Parks an iframe instance under a stable runtime id.
 *
 * @param {object} options
 * @param {string} options.id
 * @param {HTMLIFrameElement} options.iframe
 * @param {number} options.maxIframes
 * @param {number} options.ttlMs
 */
export function parkManagedIframe({ id, iframe, maxIframes, ttlMs }) {
    if (!(iframe instanceof HTMLIFrameElement)) {
        throw new Error(`parkManagedIframe(${id}): iframe must be an HTMLIFrameElement`);
    }

    lastMaxIframes = maxIframes;
    lastTtlMs = ttlMs;

    const existing = parkedById.get(id);
    if (existing && existing.iframe !== iframe) {
        existing.iframe.remove();
    }

    requireLot().append(iframe);
    parkedById.set(id, { iframe, parkedAt: nowMs() });
    evictIfNeeded(maxIframes, ttlMs);
}

/**
 * Takes a parked iframe for the given id (removes it from the parking lot).
 * @param {string} id
 * @returns {HTMLIFrameElement | null}
 */
export function takeParkedManagedIframe(id) {
    if (lastMaxIframes || lastTtlMs) {
        evictIfNeeded(lastMaxIframes, lastTtlMs);
    }
    const entry = parkedById.get(id);
    if (!entry) {
        return null;
    }
    parkedById.delete(id);
    return entry.iframe;
}

/**
 * Drops a parked iframe (hard-evict).
 * @param {string} id
 */
export function dropParkedManagedIframe(id) {
    const entry = parkedById.get(id);
    if (!entry) {
        return;
    }
    entry.iframe.remove();
    parkedById.delete(id);
}

/**
 * Returns the current number of parked iframes.
 * @returns {number}
 */
export function getParkedManagedIframeCount() {
    return parkedById.size;
}
