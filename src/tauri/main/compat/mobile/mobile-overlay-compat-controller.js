import {
    SURFACE_ATTR,
    applySurfaceContract,
    findBlockingSurfaceAncestor,
    isHostAdmittedSurface,
    shouldSkip,
} from './mobile-overlay-surface-admission.js';

const CONTROLLER_KEY = '__TAURITAVERN_MOBILE_OVERLAY_COMPAT__';

const SURFACE_SETTLE_FRAMES = 2;
const SURFACE_ATTRIBUTE_FILTER = ['class', 'style', 'hidden', 'open', 'aria-hidden'];
const FREE_WINDOW_SURFACE = 'free-window';
const INLINE_SURFACE_LIFECYCLE_STYLE_PROPERTIES = [
    'display',
    'visibility',
    'position',
    'pointer-events',
    'cursor',
    'touch-action',
];

/** @type {WeakMap<HTMLElement, number>} */
const surfaceSettleRemaining = new WeakMap();
/** @type {WeakSet<HTMLElement>} */
const settleScheduled = new WeakSet();
/** @type {WeakMap<HTMLElement, string>} */
const inlineSurfaceLifecycleStyleSignatures = new WeakMap();
/** @type {WeakSet<HTMLElement>} */
const attributeRevalidationScheduled = new WeakSet();

function readInlineSurfaceLifecycleStyleSignature(element) {
    const style = element.style;
    return INLINE_SURFACE_LIFECYCLE_STYLE_PROPERTIES
        .map((property) => `${property}:${String(style.getPropertyValue(property) || '').trim().toLowerCase()}`)
        .join(';');
}

export function installMobileOverlayCompatController() {
    if (window[CONTROLLER_KEY]) {
        return window[CONTROLLER_KEY];
    }

    if (typeof MutationObserver !== 'function') {
        throw new Error('[TauriTavern] MutationObserver unavailable while installing mobile overlay compat controller.');
    }

    const trackedSurfaces = new Set();
    const trackedPortals = new Map();
    const trackedSurfaceObservers = new Map();

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

    const revalidateSurface = (element) => {
        if (!element.isConnected) {
            unwatchSurface(element);
            return;
        }

        const settling = (surfaceSettleRemaining.get(element) ?? 0) > 0;
        applySurfaceContract(element, { settling });
        inlineSurfaceLifecycleStyleSignatures.set(element, readInlineSurfaceLifecycleStyleSignature(element));
    };

    const scheduleAttributeRevalidation = (element) => {
        if (disposed || attributeRevalidationScheduled.has(element)) {
            return;
        }

        attributeRevalidationScheduled.add(element);
        const schedule = typeof requestAnimationFrame === 'function'
            ? requestAnimationFrame
            : (handler) => handler();
        schedule(() => {
            attributeRevalidationScheduled.delete(element);
            if (disposed) {
                return;
            }
            revalidateSurface(element);
        });
    };

    const shouldRevalidateStyleMutation = (element) => {
        const currentSurface = String(element.getAttribute(SURFACE_ATTR) || '').trim();
        const wasSettling = (surfaceSettleRemaining.get(element) ?? 0) > 0;
        const previousSignature = inlineSurfaceLifecycleStyleSignatures.get(element);
        const nextSignature = readInlineSurfaceLifecycleStyleSignature(element);
        inlineSurfaceLifecycleStyleSignatures.set(element, nextSignature);

        if (currentSurface !== FREE_WINDOW_SURFACE || wasSettling) {
            return true;
        }

        // Stable free-window surfaces are user-positioned; only lifecycle style changes re-enter classification.
        return previousSignature !== nextSignature;
    };

    const shouldRevalidateAttributeMutation = (element, records) => {
        let sawStyleMutation = false;
        for (const record of records) {
            if (record.attributeName !== 'style') {
                return true;
            }
            sawStyleMutation = true;
        }

        return sawStyleMutation && shouldRevalidateStyleMutation(element);
    };

    const watchSurfaceAttributes = (element) => {
        if (trackedSurfaceObservers.has(element)) {
            return;
        }

        const observer = new MutationObserver((records) => {
            if (disposed) {
                return;
            }
            if (shouldRevalidateAttributeMutation(element, records)) {
                scheduleAttributeRevalidation(element);
            }
        });
        observer.observe(element, { attributes: true, attributeFilter: SURFACE_ATTRIBUTE_FILTER });
        trackedSurfaceObservers.set(element, observer);
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
        inlineSurfaceLifecycleStyleSignatures.set(element, readInlineSurfaceLifecycleStyleSignature(element));
        watchSurfaceAttributes(element);
        surfaceSettleRemaining.set(element, SURFACE_SETTLE_FRAMES);
        applySurfaceContract(element, { settling: true });
        inlineSurfaceLifecycleStyleSignatures.set(element, readInlineSurfaceLifecycleStyleSignature(element));
        scheduleSurfaceSettle(element);
    };

    const unwatchSurface = (element) => {
        const observer = trackedSurfaceObservers.get(element);
        if (observer) {
            observer.disconnect();
            trackedSurfaceObservers.delete(element);
        }
        trackedSurfaces.delete(element);
        surfaceSettleRemaining.delete(element);
        attributeRevalidationScheduled.delete(element);
        inlineSurfaceLifecycleStyleSignatures.delete(element);
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

        for (const observer of trackedSurfaceObservers.values()) {
            observer.disconnect();
        }
        trackedSurfaceObservers.clear();
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
            revalidateSurface(surface);
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
