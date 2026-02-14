// Application bootstrap.

window.__TAURI_RUNNING__ = true;

/**
 * Dynamic import with retry — works around Android WebView transiently failing
 * to serve modules via the asset protocol while first-launch I/O is in progress.
 */
async function importWithRetry(specifier, retries = 8, delay = 500) {
    for (let i = 0; i <= retries; i++) {
        try {
            return await import(specifier);
        } catch (error) {
            if (i === retries) throw error;
            console.warn(`TauriTavern: import('${specifier}') attempt ${i + 1} failed, retrying in ${delay}ms…`);
            await new Promise(r => setTimeout(r, delay));
        }
    }
}

async function initializeApplication() {
    try {
        // lib.js statically imports ./dist/lib.bundle.js, so this guarantees
        // all library exports are ready before loading the app.
        await importWithRetry('./lib.js');
        await importWithRetry('./tauri-main.js');
        await importWithRetry('./script.js');
    } catch (error) {
        console.error('TauriTavern: Failed to initialize application:', error);
    }
}

initializeApplication();
