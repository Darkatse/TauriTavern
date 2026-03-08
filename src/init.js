// Application bootstrap.

window.__TAURI_RUNNING__ = true;

async function setupDevThirdPartyExtensionServiceWorker() {
    if (typeof window === 'undefined') {
        return;
    }

    const protocol = window.location?.protocol || '';
    const hostname = window.location?.hostname || '';
    if (!hostname || protocol === 'tauri:' || hostname === 'tauri.localhost') {
        return;
    }

    if (!('serviceWorker' in navigator)) {
        return;
    }

    const convertFileSrc = window.__TAURI__?.core?.convertFileSrc
        || window.__TAURI_INTERNALS__?.convertFileSrc;
    if (typeof convertFileSrc !== 'function') {
        return;
    }

    const ttExtBaseUrl = String(convertFileSrc('', 'tt-ext') || '').trim();
    const swUrl = `/tt-ext-sw.js?base=${encodeURIComponent(ttExtBaseUrl)}`;

    try {
        await navigator.serviceWorker.register(swUrl, { scope: '/' });
        await navigator.serviceWorker.ready;

        if (!navigator.serviceWorker.controller) {
            await new Promise((resolve) => {
                const timeoutId = setTimeout(resolve, 1000);
                navigator.serviceWorker.addEventListener('controllerchange', () => {
                    clearTimeout(timeoutId);
                    resolve();
                }, { once: true });
            });
        }
    } catch (error) {
        console.warn('TauriTavern: Failed to enable dev third-party extension endpoint:', error);
    }
}

/**
 * Dynamic import with retry — works around Android WebView transiently failing
 * to serve modules via the asset protocol while first-launch I/O is in progress.
 */
async function importWithRetry(specifier, retries = 8, delay = 500) {
    const buildSpecifier = (attempt) => {
        if (attempt === 0) {
            return specifier;
        }

        const separator = specifier.includes('?') ? '&' : '?';
        return `${specifier}${separator}tt_retry=${attempt}&t=${Date.now()}`;
    };

    for (let i = 0; i <= retries; i++) {
        try {
            return await import(buildSpecifier(i));
        } catch (error) {
            if (i === retries) throw error;
            console.warn(`TauriTavern: import('${specifier}') attempt ${i + 1} failed, retrying in ${delay}ms…`);
            await new Promise(r => setTimeout(r, delay));
        }
    }
}

async function initializeApplication() {
    try {
        await setupDevThirdPartyExtensionServiceWorker();

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
