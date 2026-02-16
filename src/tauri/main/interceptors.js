export function createInterceptors({
    isTauri,
    originalFetch,
    canHandleRequest,
    toUrl,
    routeRequest,
    jsonResponse,
    safeJson,
}) {
    let fetchPatched = false;
    let ajaxPatched = false;

    function patchFetch() {
        if (fetchPatched) {
            return;
        }
        fetchPatched = true;

        window.fetch = async function patchedFetch(input, init = {}) {
            if (!isTauri) {
                return originalFetch(input, init);
            }

            const requestUrl = toUrl(input);
            if (!requestUrl || !canHandleRequest(requestUrl, input, init)) {
                return originalFetch(input, init);
            }

            const response = await routeRequest(requestUrl, input, init);
            return response || jsonResponse({ error: `Unsupported endpoint: ${requestUrl.pathname}` }, 404);
        };
    }

    function patchJQueryAjax() {
        if (ajaxPatched) {
            return;
        }

        const $ = window.jQuery || window.$;
        if (!$ || typeof $.ajax !== 'function') {
            return;
        }
        ajaxPatched = true;

        const originalAjax = $.ajax.bind($);

        const patchedAjax = function ajaxProxy(urlOrOptions, maybeOptions) {
            if (!isTauri) {
                return originalAjax(urlOrOptions, maybeOptions);
            }

            const options = typeof urlOrOptions === 'string'
                ? { ...(maybeOptions || {}), url: urlOrOptions }
                : { ...(urlOrOptions || {}) };

            const requestUrl = toUrl(options.url);
            if (!requestUrl || !canHandleRequest(requestUrl, options.url, {
                method: options.type || options.method || 'GET',
            })) {
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

                const response = await routeRequest(requestUrl, options.url, init);
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
        if (window.$ && window.$ !== $) {
            window.$.ajax = patchedAjax;
        }
    }

    return {
        patchFetch,
        patchJQueryAjax,
    };
}
