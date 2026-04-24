// @ts-check

const RUNTIME_SENTINEL = '__TAURITAVERN_DIALOG_POLYFILL_COVERAGE_RUNTIME__';
const BRIDGE_SENTINEL = '__TAURITAVERN_DIALOG_POLYFILL_COVERAGE_BRIDGE__';

/**
 * Installs dialog-polyfill coverage in the current window (runtime realm).
 *
 * This is intentionally a no-op when the platform already supports
 * `HTMLDialogElement.prototype.showModal`.
 */
export async function installDialogPolyfillCoverageRuntime() {
    if (globalThis[RUNTIME_SENTINEL]) {
        return;
    }
    globalThis[RUNTIME_SENTINEL] = true;

    if (!needsDialogPolyfill(/** @type {Window} */ (globalThis))) {
        return;
    }

    const root = document.documentElement;
    if (!(root instanceof HTMLElement)) {
        throw new Error('installDialogPolyfillCoverageRuntime: documentElement not available');
    }

    ensureDialogPolyfillCssLoaded();

    const module = await import('../../../../lib/dialog-polyfill.esm.js');
    const dialogPolyfill = module?.default;
    if (!dialogPolyfill) {
        throw new Error('installDialogPolyfillCoverageRuntime: dialog-polyfill default export missing');
    }

    /**
     * @param {Element} element
     */
    const registerDialog = (element) => {
        if (element.localName !== 'dialog') {
            throw new Error('installDialogPolyfillCoverageRuntime: expected <dialog> element');
        }

        element.classList.add('poly_dialog');
        dialogPolyfill.registerDialog(element);
    };

    /**
     * @param {Node} node
     */
    const scan = (node) => {
        if (!(node instanceof Element)) {
            return;
        }

        if (node.localName === 'dialog') {
            registerDialog(node);
        }

        for (const dialog of node.querySelectorAll('dialog')) {
            registerDialog(dialog);
        }
    };

    scan(root);

    const observer = new MutationObserver((records) => {
        for (const record of records) {
            for (const addedNode of record.addedNodes) {
                scan(addedNode);
            }
        }
    });
    observer.observe(root, { childList: true, subtree: true });

    window.addEventListener('beforeunload', () => observer.disconnect(), { once: true });
}

/**
 * Installs dialog-polyfill coverage for a target same-origin window.
 *
 * For the main window this runs in-process.
 * For same-origin iframes/windows this injects a tiny module script that imports
 * this module inside the target realm and installs there.
 *
 * @param {Window} [targetWindow]
 */
export function installDialogPolyfillCoverage(targetWindow = window) {
    if (!targetWindow) {
        return;
    }

    if (!needsDialogPolyfill(targetWindow)) {
        return;
    }

    if (targetWindow === window) {
        void installDialogPolyfillCoverageRuntime().catch((error) => {
            console.error('TauriTavern: Failed to install dialog polyfill coverage:', error);
        });
        return;
    }

    try {
        if (targetWindow[BRIDGE_SENTINEL]) {
            return;
        }
        targetWindow[BRIDGE_SENTINEL] = true;
    } catch {
        // If we can't store a sentinel, don't attempt to inject.
        return;
    }

    let doc = null;
    try {
        doc = targetWindow.document;
    } catch {
        return;
    }
    if (!doc) {
        return;
    }

    const script = doc.createElement('script');
    script.type = 'module';
    script.textContent = [
        `import { installDialogPolyfillCoverageRuntime } from ${JSON.stringify(import.meta.url)};`,
        `installDialogPolyfillCoverageRuntime().catch((error) => {`,
        `  console.error('TauriTavern: Failed to install dialog polyfill coverage:', error);`,
        `});`,
    ].join('\n');

    (doc.head || doc.documentElement).appendChild(script);
}

/**
 * @param {Window} targetWindow
 */
function needsDialogPolyfill(targetWindow) {
    try {
        const ctor = targetWindow.HTMLDialogElement;
        if (typeof ctor !== 'function') {
            return true;
        }

        return typeof ctor.prototype?.showModal !== 'function';
    } catch {
        return false;
    }
}

function ensureDialogPolyfillCssLoaded() {
    const cssHref = new URL('../../../../lib/dialog-polyfill.css', import.meta.url).href;

    for (const sheet of Array.from(document.styleSheets)) {
        const href = sheet.href;
        if (!href) {
            continue;
        }
        if (href === cssHref || href.endsWith('/lib/dialog-polyfill.css')) {
            return;
        }
    }

    const link = document.createElement('link');
    link.rel = 'stylesheet';
    link.href = cssHref;
    link.addEventListener('error', (event) => {
        console.error('TauriTavern: Failed to load dialog polyfill stylesheet', { href: cssHref, event });
    });

    (document.head || document.documentElement).appendChild(link);
}

