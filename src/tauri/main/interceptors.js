export function createInterceptors({
    isTauri,
    originalFetch,
    canHandleRequest,
    toUrl,
    routeRequest,
    jsonResponse,
    safeJson,
}) {
    const fetchPatchState = new WeakMap();
    const ajaxPatchState = new WeakMap();
    const OPAQUE_BASE_PROTOCOLS = new Set(['about:', 'blob:', 'data:', 'javascript:']);

    function resolveWindowBaseUrl(targetWindow) {
        try {
            const href = String(targetWindow?.location?.href || '');
            if (href) {
                const parsedHref = new URL(href, window.location.origin);
                if (!OPAQUE_BASE_PROTOCOLS.has(parsedHref.protocol)) {
                    return parsedHref.href;
                }
            }

            const origin = String(targetWindow?.location?.origin || '');
            if (origin && origin !== 'null') {
                return origin;
            }

        } catch {
            // Ignore cross-origin access failures.
        }

        return window.location.origin;
    }

    function getFetchDelegate(targetWindow) {
        const state = fetchPatchState.get(targetWindow);
        let currentFetch;
        try {
            currentFetch = targetWindow?.fetch;
        } catch {
            return null;
        }

        if (state && currentFetch === state.patchedFetch) {
            return state.delegateFetch;
        }

        if (typeof currentFetch === 'function') {
            return currentFetch.bind(targetWindow);
        }

        if (targetWindow === window && typeof originalFetch === 'function') {
            return originalFetch;
        }

        return null;
    }

    function patchFetch(targetWindow = window) {
        if (!targetWindow) {
            return;
        }

        let currentFetch;
        try {
            currentFetch = targetWindow.fetch;
        } catch {
            return;
        }

        const state = fetchPatchState.get(targetWindow);
        if (state && currentFetch === state.patchedFetch) {
            return;
        }

        const delegateFetch = getFetchDelegate(targetWindow);
        if (!delegateFetch) {
            return;
        }

        const patchedFetch = async function patchedFetch(input, init = {}) {
            if (!isTauri) {
                return delegateFetch(input, init);
            }

            const requestUrl = toUrl(input, resolveWindowBaseUrl(targetWindow));
            if (!requestUrl || !canHandleRequest(requestUrl, input, init, targetWindow)) {
                return delegateFetch(input, init);
            }

            const response = await routeRequest(requestUrl, input, init, targetWindow);
            return response || jsonResponse({ error: `Unsupported endpoint: ${requestUrl.pathname}` }, 404);
        };

        try {
            targetWindow.fetch = patchedFetch;
            fetchPatchState.set(targetWindow, { patchedFetch, delegateFetch });
        } catch {
            // Ignore non-writable or cross-origin fetch bindings.
        }
    }

    function patchJQueryAjax(targetWindow = window) {
        if (!targetWindow) {
            return;
        }

        let $;
        try {
            $ = targetWindow.jQuery || targetWindow.$;
        } catch {
            return;
        }
        if (!$ || typeof $.ajax !== 'function') {
            return;
        }

        const state = ajaxPatchState.get(targetWindow);
        if (state && state.owner === $ && $.ajax === state.patchedAjax) {
            return;
        }

        const originalAjax = $.ajax.bind($);

        const patchedAjax = function ajaxProxy(urlOrOptions, maybeOptions) {
            if (!isTauri) {
                return originalAjax(urlOrOptions, maybeOptions);
            }

            const options = typeof urlOrOptions === 'string'
                ? { ...(maybeOptions || {}), url: urlOrOptions }
                : { ...(urlOrOptions || {}) };

            const requestUrl = toUrl(options.url, resolveWindowBaseUrl(targetWindow));
            if (!requestUrl || !canHandleRequest(requestUrl, options.url, {
                method: options.type || options.method || 'GET',
            }, targetWindow)) {
                return originalAjax(urlOrOptions, maybeOptions);
            }

            const deferred = $.Deferred();
            const jqXHR = deferred.promise();
            jqXHR.abort = () => {
                // Abort is a no-op for bridged requests.
            };

            (async () => {
                const init = {
                    method: options.type || options.method || 'GET',
                    headers: options.headers,
                    body: options.data,
                };

                const response = await routeRequest(requestUrl, options.url, init, targetWindow);
                if (!response) {
                    throw new Error(`Unsupported endpoint: ${requestUrl.pathname}`);
                }

                jqXHR.status = response.status;
                jqXHR.readyState = 4;
                jqXHR.getResponseHeader = (name) => response.headers.get(name);

                const isJson = (options.dataType || '').toLowerCase() !== 'text';
                const payload = isJson ? await safeJson(response) : await response.text();
                jqXHR.responseJSON = isJson ? payload : undefined;
                jqXHR.responseText = typeof payload === 'string' ? payload : JSON.stringify(payload);

                if (response.ok) {
                    if (typeof options.success === 'function') {
                        options.success(payload, 'success', jqXHR);
                    }
                    if (typeof options.complete === 'function') {
                        options.complete(jqXHR, 'success');
                    }
                    deferred.resolve(payload, 'success', jqXHR);
                    return;
                }

                const message = typeof payload === 'string'
                    ? payload
                    : payload?.error || response.statusText;
                const error = new Error(message);

                if (typeof options.error === 'function') {
                    options.error(jqXHR, 'error', error);
                }
                if (typeof options.complete === 'function') {
                    options.complete(jqXHR, 'error');
                }

                deferred.reject(jqXHR, 'error', error);
            })().catch((error) => {
                if (typeof options.error === 'function') {
                    options.error(jqXHR, 'error', error);
                }
                if (typeof options.complete === 'function') {
                    options.complete(jqXHR, 'error');
                }
                deferred.reject(jqXHR, 'error', error);
            });

            return jqXHR;
        };

        $.ajax = patchedAjax;
        if (targetWindow.$ && targetWindow.$ !== $) {
            targetWindow.$.ajax = patchedAjax;
        }
        if (targetWindow.jQuery && targetWindow.jQuery !== $) {
            targetWindow.jQuery.ajax = patchedAjax;
        }

        ajaxPatchState.set(targetWindow, { owner: $, patchedAjax });
    }

    return {
        patchFetch,
        patchJQueryAjax,
    };
}
