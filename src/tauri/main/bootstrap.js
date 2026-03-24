import { initializeBridge, invoke, isTauri as isTauriRuntime, convertFileSrc } from '../../tauri-bridge.js';
import { createTauriMainContext } from './context.js';
import { createDownloadBridge } from './download-bridge.js';
import { createInterceptors } from './interceptors.js';
import { createRouteRegistry } from './router.js';
import { installBackNavigationBridge } from './back-navigation.js';
import { installNativeShareBridge } from './share-target-bridge.js';
import { downloadBlobWithRuntime, isNativeMobileDownloadRuntime } from '../../scripts/file-export.js';
import { showExportSuccessToast } from '../../scripts/download-feedback.js';
import { installAndroidImeLayoutHost } from './compat/mobile/android-ime-layout-host.js';
import { installMobileOverlayCompatController } from './compat/mobile/mobile-overlay-compat-controller.js';
import { installMobileRuntimeCompat } from './compat/mobile/mobile-runtime-compat.js';
import { createTraceIdFactory, DEFAULT_TRACE_HEADER } from './kernel/tracing/trace.js';
import { installBackendErrorBridge } from './bootstrap/backend-error-bridge.js';
import { installMainApiOptionParking } from './adapters/st/main-api-selector-option-parking.js';
import { installChatApi } from './api/chat.js';
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
import { isEmbeddedRuntimeTakeoverDisabled } from './services/embedded-runtime/embedded-runtime-profile-state.js';
import { preinstallPanelRuntime } from './services/panel-runtime/preinstall.js';
let bootstrapped = false;
const HOST_ABI_VERSION = 1;

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
    for (const [install, label] of [
        [installMobileRuntimeCompat, 'mobile runtime compat'],
        [installAndroidImeLayoutHost, 'Android IME layout host'],
        [installMobileOverlayCompatController, 'mobile overlay compat controller'],
    ]) {
        try {
            install();
        } catch (error) {
            console.error(`Failed to install ${label}:`, error);
        }
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

/**
 * Stable platform ABI for vendor / third-party scripts.
 *
 * Keep this object minimal: it should be an API surface, not a dumping ground.
 *
 * @param {any} context
 */
function installHostAbi(context) {
    window.__TAURITAVERN__ = {
        abiVersion: HOST_ABI_VERSION,
        traceHeader: DEFAULT_TRACE_HEADER,
        ready: null,
        invoke: {
            safeInvoke: context.safeInvoke,
            invalidate: context.invalidateInvoke,
            invalidateAll: context.invalidateInvokeAll,
            flush: context.flushInvokes,
            flushAll: context.flushAllInvokes,
            broker: context.invokeBroker,
        },
        assets: {
            thumbnailUrl: window.__TAURITAVERN_THUMBNAIL__,
            thumbnailBlobUrl: window.__TAURITAVERN_THUMBNAIL_BLOB_URL__,
            backgroundPath: window.__TAURITAVERN_BACKGROUND_PATH__,
            avatarPath: window.__TAURITAVERN_AVATAR_PATH__,
            personaPath: window.__TAURITAVERN_PERSONA_PATH__,
        },
    };
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

    const context = createTauriMainContext({ invoke, convertFileSrc });
    installHostAbi(context); installChatApi(context);
    installMainApiOptionParking();
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

    const nextTraceId = createTraceIdFactory('req');

    const canHandleRequest = (url, input, init, targetWindow = window) => {
        if (!url || url.origin !== getWindowOrigin(targetWindow)) {
            return false;
        }

        const method = getMethodHint(input, init);
        return router.canHandle(method, url.pathname);
    };

    const routeRequest = async (url, input, init, _targetWindow) => {
        const traceId = nextTraceId();
        const method = await getMethod(input, init);
        const body = await readRequestBody(input, init);
        const response = await router.handle({
            url,
            path: url.pathname,
            method,
            body,
            input,
            init,
            traceId,
        });

        const finalResponse = response || jsonResponse({ error: `Unsupported endpoint: ${url.pathname}` }, 404);
        finalResponse.headers.set(DEFAULT_TRACE_HEADER, traceId);
        return finalResponse;
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
    installSameOriginWindowPatches(interceptors, downloadBridge); preinstallPanelRuntime();
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
    if (window.__TAURITAVERN__) {
        window.__TAURITAVERN__.ready = readyPromise;
    }

    void readyPromise.then(() => import('../../scripts/tauri/setting/setting-panel.js').then(({ installTauriTavernSettingsPanel }) => installTauriTavernSettingsPanel()).catch((error) => { console.warn('TauriTavern: Failed to load settings panels:', error); }));
    void readyPromise.then(() => import('./services/chat-history/install.js').then(({ installChatHistoryMode }) => installChatHistoryMode()));
    if (!isEmbeddedRuntimeTakeoverDisabled()) void readyPromise.then(() => import('./services/embedded-runtime/install.js').then(({ installEmbeddedRuntime }) => installEmbeddedRuntime()));
    void readyPromise.then(() => import('./services/panel-runtime/install.js').then(({ installPanelRuntime }) => installPanelRuntime()));

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
