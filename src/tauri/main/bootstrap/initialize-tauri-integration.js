// @ts-check

import { invoke } from '../../../tauri-bridge.js';
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

    await installBackendErrorBridge();
    if (perfEnabled) {
        safePerfMark('tt:tauri:init:error-bridge-ready');
    }

    await invoke('wait_for_backend_ready');
    if (perfEnabled) {
        safePerfMark('tt:tauri:init:backend-ready');
    }

    // Re-apply runtime patches in case third-party code recreated fetch/jQuery or download bindings after bootstrap.
    interceptors.patchFetch();
    interceptors.patchJQueryAjax();
    downloadBridge.patchWindow();
}
