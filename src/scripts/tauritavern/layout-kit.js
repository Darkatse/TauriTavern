const SURFACE_ATTR = 'data-tt-mobile-surface';

export const SURFACE = /** @type {const} */ ({
    Backdrop: 'backdrop',
    FullscreenWindow: 'fullscreen-window',
    FreeWindow: 'free-window',
    ViewportHost: 'viewport-host',
    EdgeWindow: 'edge-window',
    None: 'none',
});

function getCandidateHostWindows() {
    /** @type {Window[]} */
    const candidates = [window];

    try {
        if (window.parent && window.parent !== window) {
            candidates.push(window.parent);
        }
    } catch {
        // Ignore cross-origin access failures.
    }

    try {
        if (window.top && window.top !== window && window.top !== window.parent) {
            candidates.push(window.top);
        }
    } catch {
        // Ignore cross-origin access failures.
    }

    return candidates;
}

export function getHostWindow() {
    for (const candidate of getCandidateHostWindows()) {
        if (candidate?.__TAURITAVERN__) {
            return candidate;
        }
    }
    return null;
}

export function getHostAbi() {
    return getHostWindow()?.__TAURITAVERN__ ?? null;
}

export function getLayoutApi() {
    return getHostAbi()?.api?.layout ?? null;
}

export function requireLayoutApi() {
    const api = getLayoutApi();
    if (!api) {
        throw new Error('[TauriTavern] Host layout API is unavailable.');
    }
    return api;
}

export async function waitForHostReady() {
    const hostWindow = getHostWindow();
    const readyPromise = hostWindow?.__TAURITAVERN__?.ready ?? hostWindow?.__TAURITAVERN_MAIN_READY__;
    if (readyPromise) {
        await readyPromise;
    }
}

export function applySurface(element, surface) {
    if (!(element instanceof Element)) {
        throw new Error('element must be an Element');
    }

    const normalized = String(surface || '').trim();
    if (!Object.values(SURFACE).includes(normalized)) {
        throw new Error(`Unsupported surface: ${normalized}`);
    }

    element.setAttribute(SURFACE_ATTR, normalized);
}

export function subscribeLayout(handler) {
    return requireLayoutApi().subscribe(handler);
}

