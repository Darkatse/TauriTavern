import {
    SURFACE_ATTR,
    applySurfaceContract,
    findBlockingSurfaceAncestor,
    isHostAdmittedSurface,
    shouldSkip,
} from './mobile-overlay-surface-admission.js';

const CONTROLLER_KEY = '__TAURITAVERN_MOBILE_OVERLAY_COMPAT__';

const SURFACE_SETTLE_FRAMES = 2;

/** @type {WeakMap<HTMLElement, number>} */
const surfaceSettleRemaining = new WeakMap();
/** @type {WeakSet<HTMLElement>} */
const settleScheduled = new WeakSet();

export function installMobileOverlayCompatController() {
    if (window[CONTROLLER_KEY]) {
        return window[CONTROLLER_KEY];
    }

    if (typeof MutationObserver !== 'function') {
        throw new Error('[TauriTavern] MutationObserver unavailable while installing mobile overlay compat controller.');
    }

    const trackedSurfaces = new Set();
    const trackedPortals = new Map();

    let bodyObserver = null;
    let disposed = false;

    const scheduleSurfaceSettle = (element) => {
        if (disposed || settleScheduled.has(element) || typeof requestAnimationFrame !== 'function') {
            return;
        }

        const remaining = surfaceSettleRemaining.get(element);
        if (!remaining || remaining <= 0) {
            return;
        }

        settleScheduled.add(element);
        requestAnimationFrame(() => {
            settleScheduled.delete(element);
            if (disposed || !element.isConnected) {
                surfaceSettleRemaining.delete(element);
                return;
            }

            const remaining = surfaceSettleRemaining.get(element) ?? 0;
            const settling = remaining > 0;
            const nextRemaining = remaining - 1;
            if (nextRemaining > 0) {
                surfaceSettleRemaining.set(element, nextRemaining);
            } else {
                surfaceSettleRemaining.delete(element);
            }

            applySurfaceContract(element, { settling });
            scheduleSurfaceSettle(element);
        });
    };

    const scanSubtree = (root, visitor) => {
        if (!(root instanceof HTMLElement)) {
            return;
        }

        visitor(root);

        const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
        while (walker.nextNode()) {
            const node = walker.currentNode;
            if (node instanceof HTMLElement) {
                visitor(node);
            }
        }
    };

    const watchPortal = (portalRoot) => {
        if (!(portalRoot instanceof HTMLElement) || trackedPortals.has(portalRoot) || shouldSkip(portalRoot)) {
            return;
        }

        if (portalRoot instanceof HTMLIFrameElement) {
            return;
        }

        if (!portalRoot.hasAttribute('script_id')) {
            return;
        }

        const observer = new MutationObserver((records) => {
            for (const record of records) {
                for (const node of record.addedNodes) {
                    scanSubtree(node, watchSurface);
                }

                for (const node of record.removedNodes) {
                    scanSubtree(node, unwatchSurface);
                }
            }
        });

        observer.observe(portalRoot, { childList: true, subtree: true });
        trackedPortals.set(portalRoot, observer);

        // Catch elements that were already mounted before the body observer ran.
        scanSubtree(portalRoot, watchSurface);
    };

    const unwatchPortal = (portalRoot) => {
        const observer = trackedPortals.get(portalRoot);
        if (!observer) {
            return;
        }
        observer.disconnect();
        trackedPortals.delete(portalRoot);

        scanSubtree(portalRoot, unwatchSurface);
    };

    const watchSurface = (element) => {
        if (!(element instanceof HTMLElement) || trackedSurfaces.has(element) || shouldSkip(element)) {
            return;
        }

        if (findBlockingSurfaceAncestor(element)) {
            return;
        }

        const declaredSurface = String(element.getAttribute(SURFACE_ATTR) || '').trim();
        if (declaredSurface && !isHostAdmittedSurface(element)) {
            return;
        }

        const computedStyle = getComputedStyle(element);
        if (computedStyle.position !== 'fixed') {
            return;
        }

        trackedSurfaces.add(element);
        surfaceSettleRemaining.set(element, SURFACE_SETTLE_FRAMES);
        applySurfaceContract(element, { settling: true });
        scheduleSurfaceSettle(element);
    };

    const unwatchSurface = (element) => {
        trackedSurfaces.delete(element);
        surfaceSettleRemaining.delete(element);
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
    };

    const stop = () => {
        disposed = true;
        bodyObserver?.disconnect();

        for (const [portalRoot, observer] of trackedPortals.entries()) {
            observer.disconnect();
            trackedPortals.delete(portalRoot);
            scanSubtree(portalRoot, unwatchSurface);
        }

        trackedSurfaces.clear();
        // WeakMaps/Sets will be cleared by GC once the surfaces are gone.

        delete window[CONTROLLER_KEY];
    };

    const revalidate = () => {
        for (const portalRoot of trackedPortals.keys()) {
            if (!portalRoot.isConnected) {
                unwatchPortal(portalRoot);
                continue;
            }
            scanSubtree(portalRoot, watchSurface);
        }

        for (const surface of trackedSurfaces.values()) {
            if (!surface.isConnected) {
                unwatchSurface(surface);
                continue;
            }
            const settling = (surfaceSettleRemaining.get(surface) ?? 0) > 0;
            applySurfaceContract(surface, { settling });
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
