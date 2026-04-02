export const SURFACE_ATTR = 'data-tt-mobile-surface';
export const HOST_ADMITTED_ATTR = 'data-tt-mobile-surface-admitted';
export const ORIGINAL_TOP_VAR = '--tt-original-top';

const SURFACE = /** @type {const} */ ({
    Backdrop: 'backdrop',
    EdgeWindow: 'edge-window',
    FullscreenWindow: 'fullscreen-window',
    FreeWindow: 'free-window',
    ViewportHost: 'viewport-host',
    None: 'none',
});

const BACKDROP_NAME_PATTERN = /(overlay|backdrop|mask)/i;
const NON_NUMERIC_TOP_VALUE_PATTERN = /^(auto|inherit|initial|unset|revert|revert-layer)$/i;

const MAX_ADMISSION_TOP_PX = 160;
const FULLSCREEN_EDGE_TOLERANCE_PX = 24;
const FULLSCREEN_EDGE_MARGIN_PX = 24;
const MAX_DRAG_SIGNAL_NODES = 48;

const FREE_WINDOW_CURSOR_VALUES = new Set(['grab', 'grabbing', 'move']);

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

export function shouldSkip(element) {
    if (element === document.body || element === document.documentElement) {
        return true;
    }

    if (SKIP_ELEMENT_IDS.has(element.id)) {
        return true;
    }

    return Boolean(element.closest(SKIP_ANCESTOR_SELECTOR));
}

export function isHostAdmittedSurface(element) {
    if (!(element instanceof HTMLElement)) {
        return false;
    }

    if (element.hasAttribute(HOST_ADMITTED_ATTR)) {
        return true;
    }

    return Boolean(String(element.style.getPropertyValue(ORIGINAL_TOP_VAR) || '').trim());
}

export function findBlockingSurfaceAncestor(element) {
    if (!(element instanceof HTMLElement)) {
        return null;
    }

    let current = element.parentElement;
    while (current) {
        const surface = String(current.getAttribute(SURFACE_ATTR) || '').trim();
        if (surface) {
            if (surface !== SURFACE.Backdrop) {
                return current;
            }
        }
        current = current.parentElement;
    }

    return null;
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

function hasDragAffordance(element, computedStyle) {
    if (!(element instanceof HTMLElement)) {
        return false;
    }

    if (element.hasAttribute('data-dragging')) {
        return true;
    }

    const cursor = String(computedStyle?.cursor || '').trim().toLowerCase();
    const touchAction = String(computedStyle?.touchAction || '').trim().toLowerCase();
    if (FREE_WINDOW_CURSOR_VALUES.has(cursor) || touchAction === 'none') {
        return true;
    }

    const queue = [];
    for (const child of element.children) {
        queue.push(child);
    }

    let visited = 0;
    while (queue.length && visited < MAX_DRAG_SIGNAL_NODES) {
        const node = queue.shift();
        if (!(node instanceof HTMLElement)) {
            continue;
        }
        visited += 1;

        const style = getComputedStyle(node);
        const childCursor = String(style.cursor || '').trim().toLowerCase();
        const childTouchAction = String(style.touchAction || '').trim().toLowerCase();
        if (FREE_WINDOW_CURSOR_VALUES.has(childCursor) || childTouchAction === 'none') {
            return true;
        }

        for (const child of node.children) {
            queue.push(child);
        }
    }

    return false;
}

function classifySurface(element) {
    if (!(element instanceof HTMLElement) || shouldSkip(element)) {
        return null;
    }

    const computedStyle = getComputedStyle(element);
    if (computedStyle.position !== 'fixed') {
        return null;
    }

    const display = String(computedStyle.display || '').trim().toLowerCase();
    if (display === 'none') {
        return null;
    }

    const pointerEvents = String(computedStyle.pointerEvents || '').trim().toLowerCase();
    if (pointerEvents === 'none') {
        return null;
    }

    const rect = element.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) {
        return null;
    }

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
        if (element instanceof HTMLIFrameElement && element.hasAttribute('script_id')) {
            return SURFACE.ViewportHost;
        }
        return hasBackdropName(element) ? SURFACE.Backdrop : SURFACE.FullscreenWindow;
    }

    if (hasDragAffordance(element, computedStyle)) {
        return SURFACE.FreeWindow;
    }

    const topPx = parsePixelValue(computedStyle.top);
    if (topPx === null || topPx < 0 || topPx > MAX_ADMISSION_TOP_PX) {
        return null;
    }

    return SURFACE.EdgeWindow;
}

function nudgeFreeWindowOnAdmission(element) {
    const { top: insetTop } = getSafeInsets();
    if (insetTop <= 0) {
        return;
    }

    const rect = element.getBoundingClientRect();
    if (rect.top >= insetTop) {
        return;
    }

    const computedStyle = getComputedStyle(element);
    const topPx = parsePixelValue(computedStyle.top);
    if (topPx === null) {
        return;
    }

    const offset = insetTop - rect.top;
    if (offset <= 0) {
        return;
    }

    element.style.setProperty('top', `${topPx + offset}px`);
}

export function applySurfaceContract(element, { settling = false } = {}) {
    const surface = classifySurface(element);
    if (!surface) {
        if (isHostAdmittedSurface(element) && element.hasAttribute(SURFACE_ATTR)) {
            element.removeAttribute(SURFACE_ATTR);
        }
        if (element.hasAttribute(HOST_ADMITTED_ATTR)) {
            element.removeAttribute(HOST_ADMITTED_ATTR);
        }
        element.style.removeProperty(ORIGINAL_TOP_VAR);
        return;
    }

    const current = String(element.getAttribute(SURFACE_ATTR) || '').trim();
    if (current !== surface) {
        element.setAttribute(SURFACE_ATTR, surface);
    }
    if (!element.hasAttribute(HOST_ADMITTED_ATTR)) {
        element.setAttribute(HOST_ADMITTED_ATTR, '1');
    }

    if (surface === SURFACE.FreeWindow) {
        element.style.removeProperty(ORIGINAL_TOP_VAR);
        nudgeFreeWindowOnAdmission(element);
        return;
    }

    if (surface !== SURFACE.EdgeWindow) {
        element.style.removeProperty(ORIGINAL_TOP_VAR);
        return;
    }

    const computedStyle = getComputedStyle(element);
    const topPx = parsePixelValue(element.style.top) ?? parsePixelValue(computedStyle.top);
    if (topPx === null || topPx < 0) {
        return;
    }

    const existingTop = String(element.style.getPropertyValue(ORIGINAL_TOP_VAR) || '').trim();
    if (!existingTop || settling) {
        element.style.setProperty(ORIGINAL_TOP_VAR, `${topPx}px`);
    }
}
