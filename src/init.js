// Application bootstrap.

window.__TAURI_RUNNING__ = true;

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

async function setupDevThirdPartyExtensionServiceWorker() {
    if (typeof window === 'undefined') {
        return;
    }

    const announceDevSwProxyBridge = () => {
        try {
            const controller = navigator.serviceWorker?.controller;
            if (controller && typeof controller.postMessage === 'function') {
                controller.postMessage({ type: 'tt-ext-proxy-ready' });
            }
        } catch {
            // Ignore.
        }
    };

    const installClientProxyBridge = () => {
        if (!('serviceWorker' in navigator)) {
            return;
        }

        const BRIDGE_KEY = '__TAURITAVERN_DEV_SW_PROXY_BRIDGE__';
        if (window[BRIDGE_KEY]) {
            return;
        }
        window[BRIDGE_KEY] = true;

        const allowedPrefixes = [
            '/thumbnail',
            '/scripts/extensions/third-party/',
            '/characters/',
            '/backgrounds/',
            '/assets/',
            '/user/images/',
            '/user/files/',
            '/User Avatars/',
            '/User%20Avatars/',
        ];

        const isAllowedUrl = (url) => {
            try {
                const parsed = new URL(String(url || '').trim());
                if (parsed.protocol !== 'tt-ext:' || parsed.hostname !== 'localhost') {
                    return false;
                }

                const pathname = parsed.pathname || '';
                return allowedPrefixes.some((prefix) => pathname === prefix || pathname.startsWith(prefix));
            } catch {
                return false;
            }
        };

        navigator.serviceWorker.addEventListener('message', (event) => {
            const data = event?.data;
            if (!data || data.type !== 'tt-ext-proxy-request') {
                return;
            }

            const port = event.ports?.[0];
            if (!port || typeof port.postMessage !== 'function') {
                return;
            }

            const url = String(data.url || '').trim();
            if (!isAllowedUrl(url)) {
                port.postMessage({ ok: false, error: 'Blocked tt-ext proxy request' });
                return;
            }

            const method = String(data.method || 'GET').toUpperCase();
            const body = data.body || undefined;

            fetch(url, { method, body, credentials: 'omit' })
                .then(async (response) => {
                    const buffer = await response.arrayBuffer();
                    const headers = Array.from(response.headers.entries());
                    port.postMessage({
                        ok: true,
                        status: response.status,
                        statusText: response.statusText,
                        headers,
                        body: buffer,
                    }, [buffer]);
                })
                .catch((error) => {
                    port.postMessage({ ok: false, error: String(error?.message || error) });
                });
        });
    };

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
        installClientProxyBridge();
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

        announceDevSwProxyBridge();
        navigator.serviceWorker.addEventListener('controllerchange', announceDevSwProxyBridge);
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
    if (PERF_ENABLED) {
        safePerfMark('tt:init:start');
    }

    try {
        if (PERF_ENABLED) {
            safePerfMark('tt:init:dev-sw:start');
        }
        await setupDevThirdPartyExtensionServiceWorker();
        if (PERF_ENABLED) {
            safePerfMark('tt:init:dev-sw:end');
            safePerfMeasure('tt:init:dev-sw', 'tt:init:dev-sw:start', 'tt:init:dev-sw:end');
        }

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
        console.error('TauriTavern: Failed to initialize application:', error);
    } finally {
        if (PERF_ENABLED) {
            safePerfMark('tt:init:end');
            safePerfMeasure('tt:init:total', 'tt:init:start', 'tt:init:end');
        }
    }
}

initializeApplication();
