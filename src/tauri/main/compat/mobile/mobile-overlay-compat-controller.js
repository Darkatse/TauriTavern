const CONTROLLER_KEY = '__TAURITAVERN_MOBILE_OVERLAY_COMPAT__';

const INSET_TOP_VAR = '--tt-inset-top';
const INSET_TOP_REFERENCE_PATTERN = /(?:--tt-inset-top|safe-area-inset-top)/i;
const NON_NUMERIC_TOP_VALUE_PATTERN = /^(auto|inherit|initial|unset|revert|revert-layer)$/i;
const ZERO_TOP_VALUE_PATTERN = /^0(?:\.0+)?(?:[a-z%]+)?$/i;

const SKIP_ELEMENT_IDS = new Set([
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
    '#sheld',
    '#form_sheld',
    '#chat',
    '#movingDivs',
    '#left-nav-panel',
    '#right-nav-panel',
    '#character_popup',
    '#world_popup',
].join(', ');

const MAX_PATCHED_TOP_PX = 120;

export function installMobileOverlayCompatController() {
    if (window[CONTROLLER_KEY]) {
        return window[CONTROLLER_KEY];
    }

    if (typeof MutationObserver !== 'function') {
        const controller = {
            dispose() {},
            revalidate() {},
        };
        window[CONTROLLER_KEY] = controller;
        return controller;
    }

    const trackedOverlays = new Map();
    let bodyObserver = null;
    let rootStyleObserver = null;
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

    const watchOverlay = (overlay) => {
        if (!(overlay instanceof HTMLElement) || trackedOverlays.has(overlay) || shouldSkip(overlay)) {
            return;
        }

        const computedStyle = getComputedStyle(overlay);
        if (computedStyle.position !== 'fixed') {
            return;
        }

        const record = {
            observer: null,
            scheduled: false,
        };

        const observer = new MutationObserver(() => {
            if (record.scheduled || disposed) {
                return;
            }

            record.scheduled = true;
            Promise.resolve().then(() => {
                record.scheduled = false;
                if (disposed) {
                    return;
                }
                patchOverlayTop(overlay);
            });
        });

        observer.observe(overlay, {
            attributes: true,
            attributeFilter: ['style', 'class'],
        });

        record.observer = observer;
        trackedOverlays.set(overlay, record);
        patchOverlayTop(overlay);
    };

    const unwatchOverlay = (overlay) => {
        const record = trackedOverlays.get(overlay);
        if (!record) {
            return;
        }

        record.observer?.disconnect();
        trackedOverlays.delete(overlay);
    };

    const onBodyMutations = (records) => {
        for (const record of records) {
            for (const node of record.addedNodes) {
                if (node instanceof HTMLElement) {
                    watchOverlay(node);
                }
            }

            for (const node of record.removedNodes) {
                if (node instanceof HTMLElement) {
                    unwatchOverlay(node);
                }
            }
        }
    };

    const start = () => {
        if (disposed) {
            return;
        }

        for (const child of Array.from(document.body.children)) {
            if (child instanceof HTMLElement) {
                watchOverlay(child);
            }
        }

        bodyObserver = new MutationObserver(onBodyMutations);
        bodyObserver.observe(document.body, { childList: true, subtree: false });

        rootStyleObserver = new MutationObserver(scheduleRevalidate);
        rootStyleObserver.observe(document.documentElement, {
            attributes: true,
            attributeFilter: ['style'],
        });

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
        rootStyleObserver?.disconnect();

        if (window.visualViewport) {
            window.visualViewport.removeEventListener('resize', scheduleRevalidate);
            window.visualViewport.removeEventListener('scroll', scheduleRevalidate);
        }

        window.removeEventListener('resize', scheduleRevalidate);
        window.removeEventListener('orientationchange', scheduleRevalidate);

        for (const overlay of trackedOverlays.keys()) {
            unwatchOverlay(overlay);
        }

        trackedOverlays.clear();
        delete window[CONTROLLER_KEY];
    };

    const revalidate = () => {
        for (const [overlay] of trackedOverlays) {
            if (!overlay.isConnected) {
                unwatchOverlay(overlay);
                continue;
            }

            patchOverlayTop(overlay);
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
    const match = value.match(/^(-?\d+(?:\.\d+)?)px$/i);
    if (!match) {
        return null;
    }

    const parsed = Number(match[1]);
    return Number.isFinite(parsed) ? parsed : null;
}

function normalizeInsetTopValue(rawValue) {
    const value = String(rawValue || '').trim();
    if (!value || INSET_TOP_REFERENCE_PATTERN.test(value)) {
        return null;
    }

    const baseValue = value.replace(/\s*!important\s*$/i, '').trim();
    if (!baseValue || NON_NUMERIC_TOP_VALUE_PATTERN.test(baseValue)) {
        return null;
    }

    const normalizedBaseValue = ZERO_TOP_VALUE_PATTERN.test(baseValue) ? '0px' : baseValue;
    return `max(var(${INSET_TOP_VAR}), ${normalizedBaseValue})`;
}

function patchOverlayTop(element) {
    if (!(element instanceof HTMLElement) || shouldSkip(element)) {
        return;
    }

    const computedStyle = getComputedStyle(element);
    if (computedStyle.position !== 'fixed') {
        return;
    }

    const computedTop = String(computedStyle.top || '').trim();
    const computedTopPx = parsePixelValue(computedTop);
    if (computedTopPx === null || computedTopPx < 0 || computedTopPx > MAX_PATCHED_TOP_PX) {
        return;
    }

    const inlineTop = String(element.style.getPropertyValue('top') || '').trim();
    const insetTopValue = normalizeInsetTopValue(inlineTop || computedTop);
    if (!insetTopValue) {
        return;
    }

    const currentTop = String(element.style.getPropertyValue('top') || '').trim();
    const currentPriority = element.style.getPropertyPriority('top') || '';
    if (currentTop === insetTopValue && currentPriority === 'important') {
        return;
    }

    element.style.setProperty('top', insetTopValue, 'important');
}
