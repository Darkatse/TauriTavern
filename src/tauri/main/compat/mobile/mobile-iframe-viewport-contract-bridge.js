const CONTROLLER_KEY = '__TAURITAVERN_MOBILE_IFRAME_VIEWPORT_CONTRACT_BRIDGE__';

const CONTRACT_VARS = [
    '--tt-inset-top',
    '--tt-inset-right',
    '--tt-inset-left',
    '--tt-inset-bottom',
    '--tt-viewport-bottom-inset',
    '--tt-base-viewport-height',
];

function requireRoot() {
    const root = document.documentElement;
    if (!(root instanceof HTMLElement)) {
        throw new Error('[TauriTavern] documentElement unavailable while bridging iframe viewport contract.');
    }
    return root;
}

function scheduleWithAnimationFrame(handler) {
    if (typeof requestAnimationFrame === 'function') {
        requestAnimationFrame(handler);
        return;
    }
    queueMicrotask(handler);
}

function readContractSnapshot() {
    const root = requireRoot();
    const style = getComputedStyle(root);

    /** @type {Array<[string, string]>} */
    const snapshot = [];
    for (const name of CONTRACT_VARS) {
        snapshot.push([name, String(style.getPropertyValue(name) || '').trim()]);
    }
    return snapshot;
}

function applySnapshotToDocument(targetDocument, snapshot) {
    const targetRoot = targetDocument?.documentElement;
    if (!(targetRoot instanceof HTMLElement)) {
        return false;
    }

    for (const [name, value] of snapshot) {
        if (!value) {
            targetRoot.style.removeProperty(name);
            continue;
        }
        targetRoot.style.setProperty(name, value);
    }

    return true;
}

function syncIframeContract(iframeElement, snapshot) {
    if (!(iframeElement instanceof HTMLIFrameElement)) {
        return false;
    }

    try {
        const doc = iframeElement.contentDocument;
        if (!doc) {
            return false;
        }

        return applySnapshotToDocument(doc, snapshot);
    } catch {
        return false;
    }
}

export function installMobileIframeViewportContractBridge() {
    const existing = window[CONTROLLER_KEY];
    if (existing) {
        return existing;
    }

    if (typeof MutationObserver !== 'function') {
        throw new Error('[TauriTavern] MutationObserver unavailable while installing iframe viewport contract bridge.');
    }

    const trackedIframes = new Set();
    let disposed = false;
    let scheduled = false;

    const reapply = () => {
        if (disposed) {
            return;
        }

        const snapshot = readContractSnapshot();
        for (const iframeElement of trackedIframes) {
            if (!iframeElement.isConnected) {
                trackedIframes.delete(iframeElement);
                continue;
            }

            syncIframeContract(iframeElement, snapshot);
        }
    };

    const scheduleReapply = () => {
        if (disposed || scheduled) {
            return;
        }

        scheduled = true;
        scheduleWithAnimationFrame(() => {
            scheduled = false;
            reapply();
        });
    };

    const watchIframe = (iframeElement) => {
        if (!(iframeElement instanceof HTMLIFrameElement)) {
            return;
        }

        trackedIframes.add(iframeElement);
        scheduleReapply();
    };

    const root = requireRoot();
    const rootObserver = new MutationObserver(scheduleReapply);
    rootObserver.observe(root, { attributes: true, attributeFilter: ['style'] });

    window.addEventListener('resize', scheduleReapply, { passive: true });
    window.addEventListener('orientationchange', scheduleReapply, { passive: true });
    window.visualViewport?.addEventListener?.('resize', scheduleReapply, { passive: true });

    scheduleReapply();

    const controller = {
        watchIframe,
        reapply,
        dispose() {
            disposed = true;
            rootObserver.disconnect();
            window.visualViewport?.removeEventListener?.('resize', scheduleReapply);
            window.removeEventListener('orientationchange', scheduleReapply);
            window.removeEventListener('resize', scheduleReapply);
            trackedIframes.clear();
            delete window[CONTROLLER_KEY];
        },
    };

    window[CONTROLLER_KEY] = controller;
    return controller;
}
