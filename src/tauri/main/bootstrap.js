import { initializeBridge, invoke, isTauri as isTauriRuntime, convertFileSrc } from '../../tauri-bridge.js';
import { createTauriMainContext } from './context.js';
import { createInterceptors } from './interceptors.js';
import { createRouteRegistry } from './router.js';
import { installNativeShareBridge } from './share-target-bridge.js';
import {
    getMethod,
    getMethodHint,
    jsonResponse,
    readRequestBody,
    safeJson,
    textResponse,
    toUrl,
} from './http-utils.js';
import { registerRoutes } from './routes/index.js';

let bootstrapped = false;
const TAURI_BACKEND_ERROR_EVENT = 'tauritavern-backend-error';
const FRONTEND_BACKEND_ERROR_EVENT = 'tauritavern:backend-error';
const BACKEND_ERROR_QUEUE_KEY = '__TAURITAVERN_BACKEND_ERROR_QUEUE__';
const BACKEND_ERROR_READY_KEY = '__TAURITAVERN_BACKEND_ERROR_CONSUMER_READY__';
const MAX_BACKEND_ERROR_QUEUE_SIZE = 50;

function getWindowOrigin(targetWindow) {
    try {
        const origin = String(targetWindow?.location?.origin || '');
        if (!origin || origin === 'null') {
            return window.location.origin;
        }

        return origin;
    } catch {
        return window.location.origin;
    }
}

function normalizeBackendErrorPayload(payload) {
    if (typeof payload === 'string') {
        const message = payload.trim();
        return message ? { message } : null;
    }

    if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
        return null;
    }

    const message = typeof payload.message === 'string' ? payload.message.trim() : '';
    return message ? { message } : null;
}

function publishBackendError(payload) {
    const normalized = normalizeBackendErrorPayload(payload);
    if (!normalized) {
        return;
    }

    if (window[BACKEND_ERROR_READY_KEY]) {
        window.dispatchEvent(new CustomEvent(FRONTEND_BACKEND_ERROR_EVENT, { detail: normalized }));
        return;
    }

    const queuedErrors = Array.isArray(window[BACKEND_ERROR_QUEUE_KEY])
        ? window[BACKEND_ERROR_QUEUE_KEY]
        : [];
    queuedErrors.push(normalized);
    if (queuedErrors.length > MAX_BACKEND_ERROR_QUEUE_SIZE) {
        queuedErrors.splice(0, queuedErrors.length - MAX_BACKEND_ERROR_QUEUE_SIZE);
    }
    window[BACKEND_ERROR_QUEUE_KEY] = queuedErrors;
}

async function installBackendErrorBridge() {
    const tauriEvent = window.__TAURI__?.event;
    if (typeof tauriEvent?.listen !== 'function') {
        return;
    }

    try {
        await tauriEvent.listen(TAURI_BACKEND_ERROR_EVENT, (event) => {
            publishBackendError(event?.payload);
        });
    } catch (error) {
        console.error('Failed to install backend error bridge:', error);
    }
}

function installSameOriginWindowInterceptors(interceptors) {
    const trackedIframes = new WeakSet();

    const patchWindow = (targetWindow) => {
        if (!targetWindow) {
            return;
        }

        if (getWindowOrigin(targetWindow) !== window.location.origin) {
            return;
        }

        interceptors.patchFetch(targetWindow);
        interceptors.patchJQueryAjax(targetWindow);
    };

    const watchIframe = (iframeElement) => {
        if (!iframeElement || trackedIframes.has(iframeElement)) {
            return;
        }

        trackedIframes.add(iframeElement);

        const patchFromIframe = () => {
            try {
                patchWindow(iframeElement.contentWindow);
            } catch {
                // Ignore cross-origin access failures.
            }
        };

        iframeElement.addEventListener('load', patchFromIframe);
        patchFromIframe();
    };

    const scanForIframes = (rootNode) => {
        if (!(rootNode instanceof Element)) {
            return;
        }

        if (rootNode instanceof HTMLIFrameElement) {
            watchIframe(rootNode);
        }

        for (const iframeElement of rootNode.querySelectorAll('iframe')) {
            watchIframe(iframeElement);
        }
    };

    scanForIframes(document.documentElement);

    const observer = new MutationObserver((records) => {
        for (const record of records) {
            for (const addedNode of record.addedNodes) {
                scanForIframes(addedNode);
            }
        }
    });
    observer.observe(document.documentElement, { childList: true, subtree: true });

    if (typeof window.open === 'function') {
        const originalOpen = window.open.bind(window);
        window.open = function patchedWindowOpen(...args) {
            const openedWindow = originalOpen(...args);
            if (!openedWindow) {
                return openedWindow;
            }

            let attempts = 0;
            const maxAttempts = 40;
            const timer = setInterval(() => {
                attempts += 1;
                if (openedWindow.closed || attempts >= maxAttempts) {
                    clearInterval(timer);
                    return;
                }

                if (getWindowOrigin(openedWindow) !== window.location.origin) {
                    return;
                }

                patchWindow(openedWindow);
                clearInterval(timer);
            }, 250);

            return openedWindow;
        };
    }

    window.addEventListener('beforeunload', () => observer.disconnect(), { once: true });
}

export function bootstrapTauriMain() {
    if (!isTauriRuntime() || bootstrapped) {
        return;
    }
    bootstrapped = true;

    installNativeShareBridge();

    const context = createTauriMainContext({ invoke, convertFileSrc });
    const router = createRouteRegistry();
    registerRoutes(router, context, { jsonResponse, textResponse });

    const canHandleRequest = (url, input, init, targetWindow = window) => {
        if (!url || url.origin !== getWindowOrigin(targetWindow)) {
            return false;
        }

        const method = getMethodHint(input, init);
        return router.canHandle(method, url.pathname);
    };

    const routeRequest = async (url, input, init, _targetWindow) => {
        const method = await getMethod(input, init);
        const body = await readRequestBody(input, init);
        return router.handle({
            url,
            path: url.pathname,
            method,
            body,
            input,
            init,
        });
    };

    const interceptors = createInterceptors({
        isTauri: true,
        originalFetch: window.fetch.bind(window),
        canHandleRequest,
        toUrl,
        routeRequest,
        jsonResponse,
        safeJson,
    });

    interceptors.patchFetch();
    interceptors.patchJQueryAjax();
    installSameOriginWindowInterceptors(interceptors);

    const readyPromise = initializeTauriIntegration(context, interceptors).catch((error) => {
        console.error('Failed to initialize Tauri integration:', error);
    });
    window.__TAURITAVERN_MAIN_READY__ = readyPromise;
}

async function initializeTauriIntegration(context, interceptors) {
    await initializeBridge();
    await installBackendErrorBridge();
    await context.initialize();

    // Re-apply runtime patches in case third-party code recreated fetch/jQuery after bootstrap.
    interceptors.patchFetch();
    interceptors.patchJQueryAjax();
}
