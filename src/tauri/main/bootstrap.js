import { initializeBridge, invoke, isTauriEnv, convertFileSrc } from '../../tauri-bridge.js';
import { createTauriMainContext } from './context.js';
import { createInterceptors } from './interceptors.js';
import { createRouteRegistry } from './router.js';
import { installNativeShareBridge } from './share-target-bridge.js';
import {
    getMethod,
    getMethodHint,
    jsonResponse,
    readRequestBody,
    safeJson,
    textResponse,
    toUrl,
} from './http-utils.js';
import { registerRoutes } from './routes/index.js';

const isTauri = isTauriEnv;
let bootstrapped = false;

export function bootstrapTauriMain() {
    if (!isTauri || bootstrapped) {
        return;
    }
    bootstrapped = true;

    installNativeShareBridge();

    const context = createTauriMainContext({ invoke, convertFileSrc });
    const router = createRouteRegistry();
    registerRoutes(router, context, { jsonResponse, textResponse });

    const canHandleRequest = (url, input, init) => {
        if (!url || url.origin !== window.location.origin) {
            return false;
        }

        const method = getMethodHint(input, init);
        return router.canHandle(method, url.pathname);
    };

    const routeRequest = async (url, input, init) => {
        const method = await getMethod(input, init);
        const body = await readRequestBody(input, init);
        return router.handle({
            url,
            path: url.pathname,
            method,
            body,
            input,
            init,
        });
    };

    const interceptors = createInterceptors({
        isTauri,
        originalFetch: window.fetch.bind(window),
        canHandleRequest,
        toUrl,
        routeRequest,
        jsonResponse,
        safeJson,
    });

    interceptors.patchFetch();
    interceptors.patchJQueryAjax();

    const readyPromise = initializeTauriIntegration(context, interceptors).catch((error) => {
        console.error('Failed to initialize Tauri integration:', error);
    });
    window.__TAURITAVERN_MAIN_READY__ = readyPromise;
}

async function initializeTauriIntegration(context, interceptors) {
    await initializeBridge();
    await context.initialize();

    // Try patching jQuery again in case it was not available during bootstrap.
    interceptors.patchJQueryAjax();
}
