// @ts-check

export async function waitForBackendReady() {
    const invoke = window.__TAURI__?.core?.invoke;
    if (typeof invoke !== 'function') {
        throw new Error('Tauri invoke is unavailable for backend readiness');
    }

    await invoke('wait_for_backend_ready');
}
