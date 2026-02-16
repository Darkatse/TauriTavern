export async function waitForTauriMainReady() {
    const readyPromise = window.__TAURITAVERN_MAIN_READY__;
    if (!readyPromise || typeof readyPromise.then !== 'function') {
        return;
    }

    try {
        await readyPromise;
    } catch {
        // Continue with fallback URL behavior.
    }
}
