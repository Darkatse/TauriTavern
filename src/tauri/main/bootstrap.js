import { initializeBridge, invoke, isTauri as isTauriRuntime, convertFileSrc } from '../../tauri-bridge.js';
import { createTauriMainContext } from './context.js';
import { createDownloadBridge } from './download-bridge.js';
import { createInterceptors } from './interceptors.js';
import { createRouteRegistry } from './router.js';
import { installBackNavigationBridge } from './back-navigation.js';
import { installNativeShareBridge } from './share-target-bridge.js';
import { installLanSyncPanel } from '../../scripts/tauri/sync/sync-panel.js';
import { downloadBlobWithRuntime, isNativeMobileDownloadRuntime } from '../../scripts/file-export.js';
import { showExportSuccessToast } from '../../scripts/download-feedback.js';
import { installMobileOverlayCompatController } from './compat/mobile/mobile-overlay-compat-controller.js';
import { installMobileRuntimeCompat } from './compat/mobile/mobile-runtime-compat.js';
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

function isPerfHudEnabled() {
    try {
        const flag = globalThis.__TAURITAVERN_PERF_ENABLED__;
        if (typeof flag === 'boolean') {
            return flag;
        }
    } catch {
        // Ignore global access failures.
    }

    try {
        if (globalThis.localStorage?.getItem('tt:perf') === '1') {
            return true;
        }
    } catch {
        // Ignore storage access failures.
    }

    try {
        const search = String(globalThis.location?.search || '');
        if (!search) {
            return false;
        }
        const params = new URLSearchParams(search);
        return params.get('ttPerf') === '1' || params.get('tt_perf') === '1';
    } catch {
        return false;
    }
}

function safePerfMark(name, detail) {
    try {
        globalThis.performance?.mark?.(name, detail ? { detail } : undefined);
    } catch {
        // Ignore unsupported mark calls.
    }
}

function safePerfMeasure(name, startMark, endMark) {
    try {
        globalThis.performance?.measure?.(name, startMark, endMark);
    } catch {
        // Ignore unsupported measure calls.
    }
}

function isMobileUserAgent() {
    // NOTE: Intentionally self-contained UA check.
    // This runs in the Tauri bootstrap composition root; importing a shared helper here risks
    // pulling in higher-level app modules (and potential side effects / cycles) too early.
    if (typeof navigator === 'undefined') {
        return false;
    }

    const userAgent = typeof navigator.userAgent === 'string' ? navigator.userAgent : '';
    if (/android|iphone|ipad|ipod/i.test(userAgent)) {
        return true;
    }

    return navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1;
}

function installTauriMobileCompat() {
    try {
        installMobileRuntimeCompat();
    } catch (error) {
        console.error('Failed to install mobile runtime compat:', error);
    }

    try {
        installMobileOverlayCompatController();
    } catch (error) {
        console.error('Failed to install mobile overlay compat controller:', error);
    }
}

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

function installSameOriginWindowPatches(interceptors, downloadBridge) {
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
        downloadBridge.patchWindow(targetWindow);
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

    const perfEnabled = isPerfHudEnabled();
    let perfReadyPromise = null;
    if (perfEnabled) {
        safePerfMark('tt:tauri:bootstrap:start');
    }

    if (isMobileUserAgent()) {
        installTauriMobileCompat();
    }

    installBackNavigationBridge();
    installNativeShareBridge();
    installLanSyncPanel();

    const context = createTauriMainContext({ invoke, convertFileSrc });
    if (perfEnabled) {
        perfReadyPromise = import('./perf/perf-hud.js')
            .then(({ installPerfHud }) => installPerfHud({ context }))
            .catch((error) => {
                console.warn('TauriTavern: Failed to load perf HUD:', error);
                return null;
            });
        window.__TAURITAVERN_PERF_READY__ = perfReadyPromise;
    }
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
    const downloadBridge = createDownloadBridge({
        isNativeMobileDownloadRuntime,
        downloadBlobWithRuntime,
        notifyDownloadResult: showExportSuccessToast,
    });

    interceptors.patchFetch();
    interceptors.patchJQueryAjax();
    downloadBridge.patchWindow();
    installSameOriginWindowPatches(interceptors, downloadBridge);

    const readyPromise = initializeTauriIntegration(
        context,
        interceptors,
        downloadBridge,
        perfEnabled,
        perfReadyPromise,
    ).catch((error) => {
        console.error('Failed to initialize Tauri integration:', error);
    });
    window.__TAURITAVERN_MAIN_READY__ = readyPromise;

    if (perfEnabled) {
        readyPromise
            .then(() => {
                safePerfMark('tt:tauri:ready');
                safePerfMeasure('tt:tauri:ready', 'tt:tauri:bootstrap:start', 'tt:tauri:ready');
            })
            .catch(() => {});
    }
}

async function initializeTauriIntegration(context, interceptors, downloadBridge, perfEnabled, perfReadyPromise) {
    if (perfEnabled && perfReadyPromise) {
        try {
            await perfReadyPromise;
        } catch {
            // Ignore perf HUD load failures.
        }
    }

    if (perfEnabled) {
        safePerfMark('tt:tauri:init:start');
    }
    await initializeBridge();
    if (perfEnabled) {
        safePerfMark('tt:tauri:init:bridge-ready');
    }
    await installBackendErrorBridge();
    if (perfEnabled) {
        safePerfMark('tt:tauri:init:error-bridge-ready');
    }
    await context.initialize();
    if (perfEnabled) {
        safePerfMark('tt:tauri:init:context-ready');
    }

    // Re-apply runtime patches in case third-party code recreated fetch/jQuery or download bindings after bootstrap.
    interceptors.patchFetch();
    interceptors.patchJQueryAjax();
    downloadBridge.patchWindow();
}
