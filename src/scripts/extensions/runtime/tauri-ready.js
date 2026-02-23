const TAURI_MAIN_READY_KEY = '__TAURITAVERN_MAIN_READY__';
const READY_PROMISE_WAIT_TIMEOUT_MS = 7000;
const READY_PROMISE_POLL_INTERVAL_MS = 50;

function isTauriRuntime() {
    if (typeof window === 'undefined') {
        return false;
    }

    return window.__TAURI_RUNNING__ === true
        || window.__TAURI_INTERNALS__ !== undefined
        || typeof window.__TAURI__?.core?.invoke === 'function';
}

function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForReadyPromiseRegistration() {
    if (typeof window === 'undefined' || !isTauriRuntime()) {
        return null;
    }

    const deadline = Date.now() + READY_PROMISE_WAIT_TIMEOUT_MS;

    while (Date.now() < deadline) {
        const readyPromise = window[TAURI_MAIN_READY_KEY];
        if (readyPromise && typeof readyPromise.then === 'function') {
            return readyPromise;
        }

        await sleep(READY_PROMISE_POLL_INTERVAL_MS);
    }

    return null;
}

export async function waitForTauriMainReady() {
    const readyPromise = await waitForReadyPromiseRegistration();
    if (!readyPromise) {
        return;
    }

    try {
        await readyPromise;
    } catch {
        // Continue with fallback URL behavior.
    }
}
