// @ts-check

const LAYOUT_SDK_VERSION = 1;

const ROOT_CONTRACT_VARS = /** @type {const} */ ({
    InsetTop: '--tt-inset-top',
    InsetRight: '--tt-inset-right',
    InsetBottom: '--tt-inset-bottom',
    InsetLeft: '--tt-inset-left',
    BaseViewportHeight: '--tt-base-viewport-height',
});

const IME_CONTRACT_VARS = /** @type {const} */ ({
    ImeBottom: '--tt-ime-bottom',
});

const IME_ATTR = /** @type {const} */ ({
    Active: 'data-tt-ime-active',
    Surface: 'data-tt-ime-surface',
});

const IME_KIND = /** @type {const} */ ({
    Composer: 'composer',
    FixedShell: 'fixed-shell',
    Dialog: 'dialog',
});

/**
 * @typedef {{ top: number; right: number; bottom: number; left: number }} LayoutInsets
 * @typedef {{ left: number; top: number; width: number; height: number; right: number; bottom: number }} LayoutFrame
 * @typedef {{
 *   activeSurface: Element | null;
 *   kind: keyof typeof IME_KIND extends never ? never : (typeof IME_KIND)[keyof typeof IME_KIND];
 *   bottom: number;
 *   viewportBottomInset: number;
 *   keyboardOffset: number;
 * }} LayoutImeSnapshot
 * @typedef {{
 *   version: number;
 *   timestampMs: number;
 *   viewport: LayoutFrame;
 *   safeInsets: LayoutInsets;
 *   safeFrame: LayoutFrame;
 *   ime: LayoutImeSnapshot;
 * }} LayoutSnapshot
 */

function requireRoot() {
    const root = document.documentElement;
    if (!(root instanceof HTMLElement)) {
        throw new Error('[TauriTavern] documentElement is unavailable while reading layout snapshot.');
    }
    return root;
}

function createInsets(top = 0, right = 0, bottom = 0, left = 0) {
    return {
        top: Math.max(0, top),
        right: Math.max(0, right),
        bottom: Math.max(0, bottom),
        left: Math.max(0, left),
    };
}

function createFrame(left = 0, top = 0, width = 0, height = 0) {
    const safeLeft = Math.max(0, left);
    const safeTop = Math.max(0, top);
    const safeWidth = Math.max(0, width);
    const safeHeight = Math.max(0, height);
    return {
        left: safeLeft,
        top: safeTop,
        width: safeWidth,
        height: safeHeight,
        right: safeLeft + safeWidth,
        bottom: safeTop + safeHeight,
    };
}

/**
 * @param {string} rawValue
 */
function parsePixelValue(rawValue) {
    const parsed = Number.parseFloat(String(rawValue || '').trim());
    return Number.isFinite(parsed) ? parsed : 0;
}

/**
 * @param {HTMLElement} element
 * @param {string} name
 */
function readCssVarPx(element, name) {
    const style = getComputedStyle(element);
    return Math.max(0, parsePixelValue(style.getPropertyValue(name)));
}

function readSafeInsets() {
    const root = requireRoot();
    return createInsets(
        readCssVarPx(root, ROOT_CONTRACT_VARS.InsetTop),
        readCssVarPx(root, ROOT_CONTRACT_VARS.InsetRight),
        readCssVarPx(root, ROOT_CONTRACT_VARS.InsetBottom),
        readCssVarPx(root, ROOT_CONTRACT_VARS.InsetLeft),
    );
}

function readViewportFrame() {
    const viewport = window.visualViewport;
    const width = Number.isFinite(viewport?.width) && viewport ? viewport.width : window.innerWidth;
    const height = Number.isFinite(viewport?.height) && viewport ? viewport.height : window.innerHeight;
    const left = Number.isFinite(viewport?.offsetLeft) && viewport ? viewport.offsetLeft : 0;
    const top = Number.isFinite(viewport?.offsetTop) && viewport ? viewport.offsetTop : 0;
    return createFrame(left, top, width, height);
}

/**
 * @param {LayoutFrame} viewport
 * @param {LayoutInsets} safeInsets
 */
function computeSafeFrame(viewport, safeInsets) {
    return createFrame(
        viewport.left + safeInsets.left,
        viewport.top + safeInsets.top,
        viewport.width - safeInsets.left - safeInsets.right,
        viewport.height - safeInsets.top - safeInsets.bottom,
    );
}

function resolveActiveImeSurface() {
    const active = document.querySelector(`[${IME_ATTR.Active}]`);
    if (!active) {
        return null;
    }

    if (!(active instanceof Element)) {
        throw new Error('[TauriTavern] IME active surface is not an Element.');
    }

    const kind = String(active.getAttribute(IME_ATTR.Surface) || '').trim();
    if (!kind) {
        throw new Error('[TauriTavern] IME active surface is missing data-tt-ime-surface.');
    }

    if (![IME_KIND.Composer, IME_KIND.FixedShell, IME_KIND.Dialog].includes(kind)) {
        throw new Error(`[TauriTavern] Unsupported IME surface kind: ${kind}`);
    }

    return { surface: active, kind };
}

/**
 * @param {LayoutInsets} safeInsets
 */
function readImeSnapshot(safeInsets) {
    const resolved = resolveActiveImeSurface();
    const activeSurface = resolved?.surface ?? null;
    const kind = resolved?.kind ?? IME_KIND.Composer;

    const imeBottom = activeSurface instanceof HTMLElement
        ? readCssVarPx(activeSurface, IME_CONTRACT_VARS.ImeBottom)
        : 0;
    const viewportBottomInset = Math.max(safeInsets.bottom, imeBottom);
    const keyboardOffset = Math.max(viewportBottomInset - safeInsets.bottom, 0);

    return {
        activeSurface,
        kind,
        bottom: imeBottom,
        viewportBottomInset,
        keyboardOffset,
    };
}

/**
 * @returns {LayoutSnapshot}
 */
function readSnapshot() {
    const timestampMs = Date.now();
    const viewport = readViewportFrame();
    const safeInsets = readSafeInsets();
    const safeFrame = computeSafeFrame(viewport, safeInsets);
    const ime = readImeSnapshot(safeInsets);

    return {
        version: LAYOUT_SDK_VERSION,
        timestampMs,
        viewport,
        safeInsets,
        safeFrame,
        ime,
    };
}

function createLayoutEmitter() {
    /** @type {Set<(snapshot: LayoutSnapshot) => void>} */
    const subscribers = new Set();

    /** @type {MutationObserver | null} */
    let rootObserver = null;
    /** @type {MutationObserver | null} */
    let imeObserver = null;
    /** @type {Element | null} */
    let trackedImeSurface = null;

    let scheduled = false;
    let started = false;

    const scheduleWithAnimationFrame = (handler) => {
        if (typeof requestAnimationFrame === 'function') {
            requestAnimationFrame(handler);
            return;
        }
        queueMicrotask(handler);
    };

    const schedule = () => {
        if (!started || scheduled) {
            return;
        }

        scheduled = true;
        scheduleWithAnimationFrame(() => {
            scheduled = false;
            flush();
        });
    };

    const updateImeObserver = (nextSurface) => {
        if (nextSurface === trackedImeSurface) {
            return;
        }

        imeObserver?.disconnect();
        imeObserver = null;
        trackedImeSurface = nextSurface;

        if (!trackedImeSurface || !(trackedImeSurface instanceof HTMLElement)) {
            return;
        }

        imeObserver = new MutationObserver(schedule);
        imeObserver.observe(trackedImeSurface, {
            attributes: true,
            attributeFilter: ['style', IME_ATTR.Active, IME_ATTR.Surface],
        });
    };

    const flush = () => {
        if (!started) {
            return;
        }

        const snapshot = readSnapshot();
        updateImeObserver(snapshot.ime.activeSurface);

        for (const handler of subscribers) {
            try {
                handler(snapshot);
            } catch (error) {
                console.error('[TauriTavern] layout subscriber failed.', error);
            }
        }
    };

    const start = () => {
        if (started || rootObserver) {
            return;
        }

        if (typeof MutationObserver !== 'function') {
            throw new Error('[TauriTavern] MutationObserver is unavailable while subscribing layout.');
        }

        started = true;

        const root = requireRoot();
        rootObserver = new MutationObserver(schedule);
        rootObserver.observe(root, { attributes: true, attributeFilter: ['style'] });

        window.addEventListener('resize', schedule, { passive: true });
        window.addEventListener('orientationchange', schedule, { passive: true });
        window.visualViewport?.addEventListener?.('resize', schedule, { passive: true });
        window.visualViewport?.addEventListener?.('scroll', schedule, { passive: true });

        document.addEventListener('focusin', schedule, true);
        document.addEventListener('focusout', schedule, true);
    };

    const stop = () => {
        started = false;
        rootObserver?.disconnect();
        imeObserver?.disconnect();
        rootObserver = null;
        imeObserver = null;
        trackedImeSurface = null;

        window.visualViewport?.removeEventListener?.('scroll', schedule);
        window.visualViewport?.removeEventListener?.('resize', schedule);
        window.removeEventListener('orientationchange', schedule);
        window.removeEventListener('resize', schedule);

        document.removeEventListener('focusin', schedule, true);
        document.removeEventListener('focusout', schedule, true);
    };

    const subscribe = async (handler) => {
        if (typeof handler !== 'function') {
            throw new Error('handler must be a function');
        }

        subscribers.add(handler);
        start();

        const snapshot = readSnapshot();
        updateImeObserver(snapshot.ime.activeSurface);
        try {
            handler(snapshot);
        } catch (error) {
            console.error('[TauriTavern] layout subscriber failed.', error);
        }

        let active = true;
        return async () => {
            if (!active) {
                return;
            }
            active = false;
            subscribers.delete(handler);
            if (subscribers.size === 0) {
                stop();
            }
        };
    };

    return {
        snapshot: readSnapshot,
        subscribe,
    };
}

/**
 * @param {any} context
 */
export function installLayoutApi(context) {
    const hostWindow = /** @type {any} */ (window);
    const hostAbi = hostWindow.__TAURITAVERN__;
    if (!hostAbi || typeof hostAbi !== 'object') {
        throw new Error('Host ABI __TAURITAVERN__ is missing');
    }

    if (!hostAbi.api || typeof hostAbi.api !== 'object') {
        hostAbi.api = {};
    }

    const emitter = createLayoutEmitter();
    hostAbi.api.layout = {
        snapshot: emitter.snapshot,
        subscribe: emitter.subscribe,
    };
}
