// Application bootstrap.

window.__TAURI_RUNNING__ = true;

// In some WebKit builds, `location.href` may omit the trailing slash for origin-only
// URLs (e.g. `tauri://localhost`). jQuery UI Tabs uses `anchor.href` vs
// `location.href` (sans hash) to decide whether a tab is local; the mismatch can
// cause it to treat hash tabs as remote and AJAX-load the current document into a
// tab panel. Normalize the URL early to match `document.baseURI`.
if (
    globalThis.location?.protocol === 'tauri:'
    && globalThis.document?.baseURI
    && globalThis.document.baseURI.endsWith('/')
    && globalThis.location.href === globalThis.document.baseURI.slice(0, -1)
) {
    globalThis.history.replaceState(null, '', globalThis.document.baseURI);
}

const PERF_ENABLED = (() => {
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
})();

globalThis.__TAURITAVERN_PERF_ENABLED__ = PERF_ENABLED;

function safePerfMark(name, detail) {
    try {
        performance?.mark?.(name, detail ? { detail } : undefined);
    } catch {
        // Ignore unsupported mark calls.
    }
}

function safePerfMeasure(name, startMark, endMark) {
    try {
        performance?.measure?.(name, startMark, endMark);
    } catch {
        // Ignore unsupported measure calls.
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
    if (PERF_ENABLED) {
        safePerfMark('tt:init:start');
    }

    try {
        await window.__TAURITAVERN_DEV_SW_READY__;

        // lib.js statically imports ./dist/lib.core.bundle.js, so this guarantees
        // core library exports are ready before loading the app. Heavy optional
        // libs are loaded on demand from ./dist/lib.optional.bundle.js.
        if (PERF_ENABLED) {
            safePerfMark('tt:init:import:lib:start');
        }
        await importWithRetry('./lib.js');
        if (PERF_ENABLED) {
            safePerfMark('tt:init:import:lib:end');
            safePerfMeasure('tt:init:import:lib', 'tt:init:import:lib:start', 'tt:init:import:lib:end');
        }

        if (PERF_ENABLED) {
            safePerfMark('tt:init:import:tauri-main:start');
        }
        await importWithRetry('./tauri-main.js');
        if (PERF_ENABLED) {
            safePerfMark('tt:init:import:tauri-main:end');
            safePerfMeasure('tt:init:import:tauri-main', 'tt:init:import:tauri-main:start', 'tt:init:import:tauri-main:end');
        }

        if (PERF_ENABLED) {
            safePerfMark('tt:init:import:app:start');
        }
        await importWithRetry('./script.js');
        if (PERF_ENABLED) {
            safePerfMark('tt:init:import:app:end');
            safePerfMeasure('tt:init:import:app', 'tt:init:import:app:start', 'tt:init:import:app:end');
        }
    } catch (error) {
        // Loading the dialog must not delay or replace the startup error.
        void importWithRetry('./scripts/tauri/startup/startup-status-overlay.js')
            .then(({ showStartupFailure }) => showStartupFailure(error));
        throw error;
    } finally {
        if (PERF_ENABLED) {
            safePerfMark('tt:init:end');
            safePerfMeasure('tt:init:total', 'tt:init:start', 'tt:init:end');
        }
    }
}

initializeApplication();
