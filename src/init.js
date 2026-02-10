// Application bootstrap.

window.__TAURI_RUNNING__ = true;

async function initializeApplication() {
    try {
        // lib.js statically imports ./dist/lib.bundle.js, so this guarantees
        // all library exports are ready before loading the app.
        await import('./lib.js');
        await import('./tauri-main.js');
        await import('./script.js');
    } catch (error) {
        console.error('TauriTavern: Failed to initialize application:', error);
    }
}

initializeApplication();
