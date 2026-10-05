import { installWindowLayout } from '../../scripts/util/window-layout.js';
import { installWindowBackdrop } from '../../scripts/window-backdrop.js';
import { invoke, isTauri as isTauriRuntime } from '../../tauri-bridge.js';
import { eventSource, event_types } from '../../scripts/events.js';
import { createTauriMainContext } from './context.js';
import { createDownloadBridge } from './download-bridge.js';
import { createInterceptors } from './interceptors.js';
import { createRouteRegistry } from './router.js';
import { installBackNavigationBridge } from './back-navigation.js';
import { installNativeShareBridge } from './share-target-bridge.js';
import { deliverBlob, deliverRemoteFile } from '../../scripts/file-export.js';
import { showExportFailureToast, showExportSuccessToast } from '../../scripts/download-feedback.js';
import { hostPlatform, isDesktopHost, isMobileHost } from '../../scripts/util/host-identity.js';
import { installMobileRuntimeCompat } from './compat/mobile/mobile-runtime-compat.js';
import { installMobileWindowOpenCompat } from './compat/mobile/mobile-window-open-compat.js';
import { installDialogPolyfillCoverage } from './compat/dialog/dialog-polyfill-coverage.js';
import { createTraceIdFactory, DEFAULT_TRACE_HEADER } from './kernel/tracing/trace.js';
import { extractErrorText, resolveHostErrorResponse } from './kernel/host-error-response.js';
import { isAbortError } from './kernel/abort-error.js';
import { installMainApiOptionParking } from './adapters/st/main-api-selector-option-parking.js';
import { installWorldInfoGlobalSelectorSelect2Enforcer } from './adapters/st/world-info-global-selector-select2-enforcer.js';
import { confirmImageDownload } from './adapters/st/image-download-popup.js';
import {
    installDesktopFullscreenShortcut,
    leaveDesktopFullscreenForShutdown,
} from './adapters/window/desktop-fullscreen-shortcut.js';
import { installChatApi } from './api/chat.js';
import { installChatSurfaceApi } from './api/chat-surface.js';
import { installCharacterCardsApi } from './api/character-cards.js';
import { installAgentApi } from './api/agent.js';
import { installDevApi } from './api/dev.js';
import { installExtensionStoreApi } from './api/extension-store.js';
import { installDbApi } from './api/db.js';
import { installLayoutApi } from './api/layout.js';
import { installLlmConnectionsApi } from './api/llm-connection.js';
import { installMcpApi } from './api/mcp.js';
import { installSkillApi } from './api/skill.js';
import { installWorldInfoApi } from './api/world-info.js';
import { initializeTauriIntegration } from './bootstrap/initialize-tauri-integration.js';
import {
    getMethod,
    getMethodHint,
    jsonResponse,
    readRequestBody,
    textResponse,
    toUrl,
} from './http-utils.js';
import { registerRoutes } from './routes/index.js';
import { isEmbeddedRuntimeTakeoverDisabled } from './services/embedded-runtime/embedded-runtime-profile-state.js';
import { installFrontendLogCapture, setFrontendLogBackendForwardingEnabled } from './services/dev-logging/frontend-log-capture.js';
import { registerLifecycleFlushHandler } from './services/lifecycle/lifecycle-flush-service.js';
import { preinstallPanelRuntime } from './services/panel-runtime/preinstall.js';
let bootstrapped = false;
const HOST_ABI_VERSION = 2;

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
            backgroundPath: window.__TAURITAVERN_BACKGROUND_PATH__,
        },
    };
}

function installSameOriginWindowPatches(interceptors, downloadBridge, { runtimeCompat } = {}) {
    const trackedIframes = new WeakSet();

    const patchWindow = (targetWindow) => {
        if (!targetWindow || getWindowOrigin(targetWindow) !== window.location.origin) {
            return;
        }

        runtimeCompat?.(targetWindow);

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
    const isMobile = isMobileHost();
    if (isMobile) installMobileRuntimeCompat();

    installFrontendLogCapture();
    installDialogPolyfillCoverage();
    if (isDesktopHost()) {
        installDesktopFullscreenShortcut();
        registerLifecycleFlushHandler('desktop-fullscreen', leaveDesktopFullscreenForShutdown);
    }

    installBackNavigationBridge();
    installNativeShareBridge();

    const context = createTauriMainContext({ invoke });
    installHostAbi(context); installLayoutApi(); installChatApi(context); installChatSurfaceApi(); installCharacterCardsApi(context); installAgentApi(context); installLlmConnectionsApi(context); installMcpApi(context); installSkillApi(context); installDevApi(context); installExtensionStoreApi(context); installDbApi(context); installWorldInfoApi();
    installMainApiOptionParking();
    installWorldInfoGlobalSelectorSelect2Enforcer();
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
        const startedAt = globalThis.performance?.now?.() ?? Date.now();
        const traceId = nextTraceId();
        let method = 'GET';
        try {
            method = await getMethod(input, init);
            const body = await readRequestBody(input, init, router.bodyMode(method, url.pathname));
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
            const durationMs = (globalThis.performance?.now?.() ?? Date.now()) - startedAt;
            return finalResponse;
        } catch (error) {
            if (isAbortError(error)) {
                throw error;
            }

            const message = extractErrorText(error);
            const resolved = resolveHostErrorResponse(message);
            const finalResponse = textResponse(resolved.body, resolved.status);
            finalResponse.headers.set(DEFAULT_TRACE_HEADER, traceId);
            const durationMs = (globalThis.performance?.now?.() ?? Date.now()) - startedAt;
            console.error('TauriTavern route handler failed', {
                traceId,
                method,
                path: url.pathname,
                durationMs,
                message,
                error,
            });
            return finalResponse;
        }
    };

    const interceptors = createInterceptors({
        isTauri: true,
        originalFetch: window.fetch.bind(window),
        canHandleRequest,
        toUrl,
        routeRequest,
        jsonResponse,
    });
    const downloadBridge = createDownloadBridge({
        deliverBlob,
        deliverRemoteFile,
        notifyDownloadResult: showExportSuccessToast,
        notifyDownloadError: showExportFailureToast,
        // Android WebView has no image context menu, so the bridge supplies that default action.
        confirmImageDownload: hostPlatform() === 'android' ? confirmImageDownload : null,
    });

    interceptors.patchFetch();
    interceptors.patchJQueryAjax();
    downloadBridge.patchWindow();
    const runtimeCompat = (targetWindow) => {
        installDialogPolyfillCoverage(targetWindow);
        if (isMobile) {
            installMobileRuntimeCompat(targetWindow);
        } else {
            installDesktopFullscreenShortcut(targetWindow);
        }
    };
    installSameOriginWindowPatches(interceptors, downloadBridge, {
        runtimeCompat,
    });
    if (isMobile) installMobileWindowOpenCompat();
    preinstallPanelRuntime();
    const readyPromise = initializeTauriIntegration(
        context,
        interceptors,
        downloadBridge,
        perfEnabled,
        perfReadyPromise,
        safePerfMark,
    );
    void readyPromise.catch((error) => {
        console.error('Failed to initialize Tauri integration:', error);
    });
    const runAfterTauriReady = (callback) => {
        void readyPromise.then(callback, () => {});
    };
    window.__TAURITAVERN_MAIN_READY__ = readyPromise;
    if (window.__TAURITAVERN__) {
        window.__TAURITAVERN__.ready = readyPromise;
    }

    runAfterTauriReady(() => setFrontendLogBackendForwardingEnabled(true));
    runAfterTauriReady(() => {
        installWindowBackdrop(context);
        void installWindowLayout(context).catch(error => console.error('[TauriTavern] Window layout:', error));
    });

    runAfterTauriReady(() => import('../../scripts/tauri/setting/setting-panel.js')
        .then(({ installTauriTavernSettingsPanel }) => installTauriTavernSettingsPanel())
        .catch((error) => { console.warn('TauriTavern: Failed to load settings panels:', error); }));
    // This panel imports application state as well as rendering it.
    eventSource.once(event_types.APP_READY, () => {
        void import('../../scripts/tauri/generation-params/panel.js')
            .then(({ installGenerationParamsPanel }) => installGenerationParamsPanel())
            .catch((error) => { console.error('TauriTavern: Failed to install generation parameter panel:', error); });
    });
    runAfterTauriReady(() => import('./services/dynamic-theme/install.js')
        .then(({ installDynamicTheme }) => installDynamicTheme()));
    if (!isEmbeddedRuntimeTakeoverDisabled()) {
        runAfterTauriReady(() => import('./services/embedded-runtime/install.js')
            .then(({ installEmbeddedRuntime }) => installEmbeddedRuntime()));
    }
    runAfterTauriReady(() => import('./services/panel-runtime/install.js')
        .then(({ installPanelRuntime }) => installPanelRuntime()));

    if (perfEnabled) {
        readyPromise
            .then(() => {
                safePerfMark('tt:tauri:ready');
                safePerfMeasure('tt:tauri:ready', 'tt:tauri:bootstrap:start', 'tt:tauri:ready');
            })
            .catch(() => {});
    }
}
