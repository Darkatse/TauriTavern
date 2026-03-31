const CONTROLLER_KEY = '__TAURITAVERN_MOBILE_OVERLAY_COMPAT__';

const SURFACE_ATTR = 'data-tt-mobile-surface';
const ORIGINAL_TOP_VAR = '--tt-original-top';

const SURFACE = /** @type {const} */ ({
    Backdrop: 'backdrop',
    EdgeWindow: 'edge-window',
    FullscreenWindow: 'fullscreen-window',
    None: 'none',
});

const BACKDROP_NAME_PATTERN = /(overlay|backdrop|mask)/i;
const NON_NUMERIC_TOP_VALUE_PATTERN = /^(auto|inherit|initial|unset|revert|revert-layer)$/i;

const MAX_ADMISSION_TOP_PX = 160;
const FULLSCREEN_EDGE_TOLERANCE_PX = 24;
const FULLSCREEN_EDGE_MARGIN_PX = 24;

const SKIP_ELEMENT_IDS = new Set([
    'preloader',
    'bg1',
    'bg_custom',
    'character_context_menu',
    'top-settings-holder',
    'top-bar',
    'sheld',
    'form_sheld',
    'chat',
    'movingDivs',
    'left-nav-panel',
    'right-nav-panel',
    'character_popup',
    'world_popup',
]);

const SKIP_ANCESTOR_SELECTOR = [
    '#character_context_menu',
    '#top-settings-holder',
    '#top-bar',
    '#sheld',
    '#form_sheld',
    '#chat',
    '#movingDivs',
    '#left-nav-panel',
    '#right-nav-panel',
    '#character_popup',
    '#world_popup',
].join(', ');

export function installMobileOverlayCompatController() {
    if (window[CONTROLLER_KEY]) {
        return window[CONTROLLER_KEY];
    }

    if (typeof MutationObserver !== 'function') {
        throw new Error('[TauriTavern] MutationObserver unavailable while installing mobile overlay compat controller.');
    }

    if (typeof requestAnimationFrame !== 'function') {
        throw new Error('[TauriTavern] requestAnimationFrame unavailable while installing mobile overlay compat controller.');
    }

    const trackedSurfaces = new Map();
    const trackedPortals = new Map();

    let bodyObserver = null;
    let scheduledRevalidate = false;
    let disposed = false;

    const scheduleRevalidate = () => {
        if (scheduledRevalidate || disposed) {
            return;
        }

        scheduledRevalidate = true;
        requestAnimationFrame(() => {
            scheduledRevalidate = false;
            revalidate();
        });
    };

    const watchPortal = (portalRoot) => {
        if (!(portalRoot instanceof HTMLElement) || trackedPortals.has(portalRoot) || shouldSkip(portalRoot)) {
            return;
        }

        if (!portalRoot.hasAttribute('script_id')) {
            return;
        }

        const observer = new MutationObserver(() => {
            // Fixed overlays are often added after the root is appended to <body>.
            // Scanning the portal subtree is contained and keeps us off the main app tree.
            scheduleRevalidate();
        });

        observer.observe(portalRoot, { childList: true, subtree: true });
        trackedPortals.set(portalRoot, observer);

        // Catch elements that were already mounted before the body observer ran.
        scanPortal(portalRoot);
    };

    const unwatchPortal = (portalRoot) => {
        const observer = trackedPortals.get(portalRoot);
        if (!observer) {
            return;
        }
        observer.disconnect();
        trackedPortals.delete(portalRoot);
    };

    const watchSurface = (element) => {
        if (!(element instanceof HTMLElement) || trackedSurfaces.has(element) || shouldSkip(element)) {
            return;
        }

        // Explicit opt-in: if an element already declares a surface kind, don't override it.
        if (element.hasAttribute(SURFACE_ATTR)) {
            return;
        }

        const computedStyle = getComputedStyle(element);
        if (computedStyle.position !== 'fixed') {
            return;
        }

        const record = {
            observer: null,
        };

        const observer = new MutationObserver(scheduleRevalidate);
        observer.observe(element, {
            attributes: true,
            attributeFilter: ['style', 'class'],
        });

        record.observer = observer;
        trackedSurfaces.set(element, record);
        applySurfaceContract(element);
    };

    const unwatchSurface = (element) => {
        const record = trackedSurfaces.get(element);
        if (!record) {
            return;
        }
        record.observer?.disconnect();
        trackedSurfaces.delete(element);
    };

    const scanPortal = (portalRoot) => {
        if (!(portalRoot instanceof HTMLElement) || shouldSkip(portalRoot)) {
            return;
        }

        for (const node of portalRoot.querySelectorAll('*')) {
            if (node instanceof HTMLElement) {
                watchSurface(node);
            }
        }
    };

    const scanBodyChild = (node) => {
        if (!(node instanceof HTMLElement)) {
            return;
        }

        watchPortal(node);
        watchSurface(node);
    };

    const onBodyMutations = (records) => {
        for (const record of records) {
            for (const node of record.addedNodes) {
                scanBodyChild(node);
            }
            for (const node of record.removedNodes) {
                if (node instanceof HTMLElement) {
                    unwatchPortal(node);
                    unwatchSurface(node);
                }
            }
        }
    };

    const start = () => {
        if (disposed) {
            return;
        }

        if (!(document.body instanceof HTMLBodyElement)) {
            throw new Error('[TauriTavern] document.body unavailable while installing mobile overlay compat controller.');
        }

        for (const child of Array.from(document.body.children)) {
            scanBodyChild(child);
        }

        bodyObserver = new MutationObserver(onBodyMutations);
        bodyObserver.observe(document.body, { childList: true, subtree: false });

        if (window.visualViewport) {
            window.visualViewport.addEventListener('resize', scheduleRevalidate, { passive: true });
            window.visualViewport.addEventListener('scroll', scheduleRevalidate, { passive: true });
        }

        window.addEventListener('resize', scheduleRevalidate, { passive: true });
        window.addEventListener('orientationchange', scheduleRevalidate, { passive: true });
    };

    const stop = () => {
        disposed = true;
        bodyObserver?.disconnect();

        if (window.visualViewport) {
            window.visualViewport.removeEventListener('resize', scheduleRevalidate);
            window.visualViewport.removeEventListener('scroll', scheduleRevalidate);
        }

        window.removeEventListener('resize', scheduleRevalidate);
        window.removeEventListener('orientationchange', scheduleRevalidate);

        for (const [portalRoot, observer] of trackedPortals.entries()) {
            observer.disconnect();
            trackedPortals.delete(portalRoot);
        }

        for (const [surface, record] of trackedSurfaces.entries()) {
            record.observer?.disconnect();
            trackedSurfaces.delete(surface);
        }

        delete window[CONTROLLER_KEY];
    };

    const revalidate = () => {
        for (const portalRoot of trackedPortals.keys()) {
            if (!portalRoot.isConnected) {
                unwatchPortal(portalRoot);
                continue;
            }
            scanPortal(portalRoot);
        }

        for (const surface of trackedSurfaces.keys()) {
            if (!surface.isConnected) {
                unwatchSurface(surface);
                continue;
            }
            applySurfaceContract(surface);
        }
    };

    if (document.body) {
        start();
    } else {
        document.addEventListener('DOMContentLoaded', start, { once: true });
    }

    const controller = {
        dispose: stop,
        revalidate,
    };

    window[CONTROLLER_KEY] = controller;
    return controller;
}

function shouldSkip(element) {
    if (element === document.body || element === document.documentElement) {
        return true;
    }

    if (SKIP_ELEMENT_IDS.has(element.id)) {
        return true;
    }

    return Boolean(element.closest(SKIP_ANCESTOR_SELECTOR));
}

function parsePixelValue(rawValue) {
    const value = String(rawValue || '').trim();
    if (!value || NON_NUMERIC_TOP_VALUE_PATTERN.test(value)) {
        return null;
    }

    const match = value.match(/^(-?\d+(?:\.\d+)?)px$/i);
    if (!match) {
        return null;
    }

    const parsed = Number(match[1]);
    return Number.isFinite(parsed) ? parsed : null;
}

function getViewportSize() {
    const viewport = window.visualViewport;
    const width = viewport?.width ?? window.innerWidth;
    const height = viewport?.height ?? window.innerHeight;
    return {
        width: Number.isFinite(width) ? width : 0,
        height: Number.isFinite(height) ? height : 0,
    };
}

function getSafeInsets() {
    const root = document.documentElement;
    if (!(root instanceof HTMLElement)) {
        return { top: 0, left: 0, right: 0, bottom: 0 };
    }

    const style = getComputedStyle(root);

    const top = parsePixelValue(style.getPropertyValue('--tt-inset-top')) ?? 0;
    const left = parsePixelValue(style.getPropertyValue('--tt-inset-left')) ?? 0;
    const right = parsePixelValue(style.getPropertyValue('--tt-inset-right')) ?? 0;
    const bottom =
        parsePixelValue(style.getPropertyValue('--tt-viewport-bottom-inset')) ??
        parsePixelValue(style.getPropertyValue('--tt-inset-bottom')) ??
        0;

    return { top, left, right, bottom };
}

function hasBackdropName(element) {
    const id = String(element.id || '');
    const className = String(element.className || '');
    return BACKDROP_NAME_PATTERN.test(id) || BACKDROP_NAME_PATTERN.test(className);
}

function hasZeroInsetEdges(computedStyle) {
    const top = parsePixelValue(computedStyle.top);
    if (top !== 0) {
        return false;
    }

    const left = parsePixelValue(computedStyle.left);
    const right = parsePixelValue(computedStyle.right);
    const bottom = parsePixelValue(computedStyle.bottom);
    return left === 0 && right === 0 && bottom === 0;
}

function classifySurface(element) {
    if (!(element instanceof HTMLElement) || shouldSkip(element)) {
        return null;
    }

    const computedStyle = getComputedStyle(element);
    if (computedStyle.position !== 'fixed') {
        return null;
    }

    const topPx = parsePixelValue(computedStyle.top);
    if (topPx === null || topPx < 0 || topPx > MAX_ADMISSION_TOP_PX) {
        return null;
    }

    const rect = element.getBoundingClientRect();
    const viewport = getViewportSize();
    const insets = getSafeInsets();

    if (viewport.width <= 0 || viewport.height <= 0) {
        return null;
    }

    const safeWidth = Math.max(viewport.width - insets.left - insets.right, 0);
    const safeHeight = Math.max(viewport.height - insets.top - insets.bottom, 0);
    if (safeWidth <= 0 || safeHeight <= 0) {
        return null;
    }

    const isNearEdges =
        rect.top <= insets.top + FULLSCREEN_EDGE_MARGIN_PX &&
        rect.left <= insets.left + FULLSCREEN_EDGE_MARGIN_PX &&
        viewport.width - rect.right <= insets.right + FULLSCREEN_EDGE_MARGIN_PX &&
        viewport.height - rect.bottom <= insets.bottom + FULLSCREEN_EDGE_MARGIN_PX;

    const isEdgeCovering =
        rect.width >= safeWidth - FULLSCREEN_EDGE_TOLERANCE_PX &&
        rect.height >= safeHeight - FULLSCREEN_EDGE_TOLERANCE_PX;

    if (isNearEdges && isEdgeCovering) {
        return hasBackdropName(element) ? SURFACE.Backdrop : SURFACE.FullscreenWindow;
    }

    return SURFACE.EdgeWindow;
}

function applySurfaceContract(element) {
    const surface = classifySurface(element);
    if (!surface) {
        if (element.hasAttribute(SURFACE_ATTR)) {
            element.removeAttribute(SURFACE_ATTR);
        }
        element.style.removeProperty(ORIGINAL_TOP_VAR);
        return;
    }

    const current = String(element.getAttribute(SURFACE_ATTR) || '').trim();
    if (current !== surface) {
        element.setAttribute(SURFACE_ATTR, surface);
    }

    if (surface !== SURFACE.EdgeWindow) {
        element.style.removeProperty(ORIGINAL_TOP_VAR);
        return;
    }

    const existingTop = String(element.style.getPropertyValue(ORIGINAL_TOP_VAR) || '').trim();
    if (existingTop) {
        return;
    }

    const computedStyle = getComputedStyle(element);
    const topPx = parsePixelValue(computedStyle.top);
    if (topPx === null || topPx < 0) {
        throw new Error('[TauriTavern] Failed to resolve edge-window top for overlay surface.');
    }

    element.style.setProperty(ORIGINAL_TOP_VAR, `${topPx}px`);
}
