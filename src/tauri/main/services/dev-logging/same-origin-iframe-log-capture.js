// @ts-check

/** @typedef {(level: 'debug' | 'info' | 'warn' | 'error', message: string, target?: string) => void} PushFn */

/**
 * @param {HTMLIFrameElement} iframe
 */
function describeIframeTarget(iframe) {
    const id = String(iframe.id || '').trim();
    if (id) {
        return `iframe#${id}`;
    }

    const name = String(iframe.name || '').trim();
    if (name) {
        return `iframe:${name}`;
    }

    const title = String(iframe.title || '').trim();
    if (title) {
        return `iframe:${title}`;
    }

    const src = String(iframe.getAttribute('src') || '').trim();
    if (src) {
        return `iframe:${src}`;
    }

    if (String(iframe.srcdoc || '').trim()) {
        return 'iframe:srcdoc';
    }

    return 'iframe';
}

/**
 * @param {HTMLIFrameElement} iframe
 */
function isSameOriginIframe(iframe) {
    if (iframe.hasAttribute('sandbox') && !iframe.sandbox.contains('allow-same-origin')) {
        return false;
    }

    return true;
}

/**
 * @param {object} options
 * @param {PushFn} options.push
 * @param {(args: any[]) => string} options.formatConsoleArgs
 * @param {() => boolean} options.isConsoleCaptureEnabled
 */
export function createSameOriginIframeLogCapture({ push, formatConsoleArgs, isConsoleCaptureEnabled }) {
    /** @type {WeakMap<Window, Partial<Record<keyof Console, (...args: any[]) => void>>>} */
    const originalConsoleByWindow = new WeakMap();
    /** @type {WeakSet<Window>} */
    const errorCaptureInstalled = new WeakSet();
    let observerInstalled = false;

    /**
     * @param {Window} win
     * @param {() => string} getTarget
     */
    function installErrorCapture(win, getTarget) {
        if (errorCaptureInstalled.has(win)) {
            return;
        }
        errorCaptureInstalled.add(win);

        win.addEventListener('error', (event) => {
            const message = String(event?.message || 'Unknown error');
            const errorStack = event?.error && typeof event.error === 'object' ? event.error.stack : null;
            const errorMessage =
                event?.error && typeof event.error === 'object' ? event.error.message : null;
            const details =
                typeof errorStack === 'string'
                    ? `\n${errorStack}`
                    : typeof errorMessage === 'string'
                        ? `\n${errorMessage}`
                        : '';
            push('error', `${message}${details}`, getTarget());
        });

        win.addEventListener('unhandledrejection', (event) => {
            const reason = event?.reason;
            const stack = reason && typeof reason === 'object' ? reason.stack : null;
            const message = reason && typeof reason === 'object' ? reason.message : null;
            if (typeof stack === 'string' && stack) {
                push('error', `Unhandled rejection: ${stack}`, getTarget());
                return;
            }
            if (typeof message === 'string' && message) {
                push('error', `Unhandled rejection: ${message}`, getTarget());
                return;
            }
            push('error', `Unhandled rejection: ${String(reason)}`, getTarget());
        });
    }

    /**
     * @param {Window} win
     * @param {() => string} getTarget
     */
    function patchConsole(win, getTarget) {
        if (originalConsoleByWindow.has(win)) {
            return;
        }

        const consoleRef = /** @type {Console | undefined} */ (/** @type {any} */ (win).console);
        if (!consoleRef) {
            return;
        }

        const original = {
            debug: consoleRef.debug?.bind(consoleRef),
            log: consoleRef.log?.bind(consoleRef),
            info: consoleRef.info?.bind(consoleRef),
            warn: consoleRef.warn?.bind(consoleRef),
            error: consoleRef.error?.bind(consoleRef),
        };
        originalConsoleByWindow.set(win, original);

        if (original.debug) {
            consoleRef.debug = (...args) => {
                original.debug?.(...args);
                push('debug', formatConsoleArgs(args), getTarget());
            };
        }

        if (original.log) {
            consoleRef.log = (...args) => {
                original.log?.(...args);
                push('info', formatConsoleArgs(args), getTarget());
            };
        }

        if (original.info) {
            consoleRef.info = (...args) => {
                original.info?.(...args);
                push('info', formatConsoleArgs(args), getTarget());
            };
        }

        if (original.warn) {
            consoleRef.warn = (...args) => {
                original.warn?.(...args);
                push('warn', formatConsoleArgs(args), getTarget());
            };
        }

        if (original.error) {
            consoleRef.error = (...args) => {
                original.error?.(...args);
                push('error', formatConsoleArgs(args), getTarget());
            };
        }
    }

    /**
     * @param {Window} win
     */
    function restoreConsole(win) {
        const original = originalConsoleByWindow.get(win);
        const consoleRef = /** @type {Console | undefined} */ (/** @type {any} */ (win).console);
        if (!original || !consoleRef) {
            return;
        }

        if (original.debug) consoleRef.debug = original.debug;
        if (original.log) consoleRef.log = original.log;
        if (original.info) consoleRef.info = original.info;
        if (original.warn) consoleRef.warn = original.warn;
        if (original.error) consoleRef.error = original.error;

        originalConsoleByWindow.delete(win);
    }

    /**
     * @param {HTMLIFrameElement} iframe
     */
    function tryInstall(iframe) {
        if (!isSameOriginIframe(iframe)) {
            return;
        }

        if (iframe.dataset.ttDevlogIframeHooked !== '1') {
            iframe.dataset.ttDevlogIframeHooked = '1';
            iframe.addEventListener('load', () => {
                tryInstall(iframe);
            });
        }

        const win = iframe.contentWindow;
        if (!win || !iframe.contentDocument) {
            return;
        }

        const getTarget = () => describeIframeTarget(iframe);
        installErrorCapture(win, getTarget);
        if (isConsoleCaptureEnabled()) {
            patchConsole(win, getTarget);
        }
    }

    function scan() {
        for (const iframe of document.querySelectorAll('iframe')) {
            if (iframe instanceof HTMLIFrameElement) {
                tryInstall(iframe);
            }
        }
    }

    function restore() {
        for (const iframe of document.querySelectorAll('iframe')) {
            if (!(iframe instanceof HTMLIFrameElement)) {
                continue;
            }
            const win = iframe.contentWindow;
            if (win && iframe.contentDocument) {
                restoreConsole(win);
            }
        }
    }

    function installObserver() {
        if (observerInstalled) {
            return;
        }
        observerInstalled = true;

        const observer = new MutationObserver((mutations) => {
            for (const mutation of mutations) {
                for (const node of mutation.addedNodes) {
                    if (node instanceof HTMLIFrameElement) {
                        tryInstall(node);
                        continue;
                    }
                    if (node instanceof HTMLElement) {
                        for (const iframe of node.querySelectorAll('iframe')) {
                            if (iframe instanceof HTMLIFrameElement) {
                                tryInstall(iframe);
                            }
                        }
                    }
                }
            }
        });

        observer.observe(document.documentElement, { childList: true, subtree: true });
    }

    return Object.freeze({
        install: () => {
            installObserver();
            scan();
        },
        scan,
        restore,
    });
}
