// @ts-check

import { initializeBridge } from '../../../tauri-bridge.js';
import { installBackendErrorBridge } from './backend-error-bridge.js';

export async function initializeTauriIntegration(
    context,
    interceptors,
    downloadBridge,
    perfEnabled,
    perfReadyPromise,
    safePerfMark,
) {
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

