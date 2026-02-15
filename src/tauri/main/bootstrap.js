import { initializeBridge, invoke, isTauriEnv, convertFileSrc } from '../../tauri-bridge.js';
import { createTauriMainContext } from './context.js';
import { createInterceptors } from './interceptors.js';
import { createRouteRegistry } from './router.js';
import { installNativeShareBridge } from './share-target-bridge.js';
import {
    getMethod,
    jsonResponse,
    readRequestBody,
    safeJson,
    shouldIntercept,
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
        shouldIntercept,
        toUrl,
        routeRequest,
        jsonResponse,
        safeJson,
    });

    interceptors.patchFetch();
    interceptors.patchJQueryAjax();

    initializeTauriIntegration(context, interceptors).catch((error) => {
        console.error('Failed to initialize Tauri integration:', error);
    });
}

async function initializeTauriIntegration(context, interceptors) {
    await initializeBridge();
    await context.initialize();

    // Try patching jQuery again in case it was not available during bootstrap.
    interceptors.patchJQueryAjax();
}
