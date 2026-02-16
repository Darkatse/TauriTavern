async function withTimeout(taskFactory, timeoutMs, timeoutErrorFactory) {
    return await new Promise((resolve, reject) => {
        let settled = false;
        const timeoutId = setTimeout(() => {
            if (settled) {
                return;
            }
            settled = true;
            reject(timeoutErrorFactory());
        }, timeoutMs);

        Promise.resolve()
            .then(taskFactory)
            .then(result => {
                if (settled) {
                    return;
                }
                settled = true;
                clearTimeout(timeoutId);
                resolve(result);
            })
            .catch(error => {
                if (settled) {
                    return;
                }
                settled = true;
                clearTimeout(timeoutId);
                reject(error);
            });
    });
}

function toStyleLoadError(name, url, error) {
    if (error instanceof Error) {
        return error;
    }

    const errorType = error?.type || 'error';
    return new Error(`Extension "${name}" stylesheet load failed (${errorType}): ${url}`);
}

function toStyleTimeoutError(name, url, timeoutMs) {
    return new Error(`Extension "${name}" stylesheet load timed out after ${timeoutMs}ms: ${url}`);
}

function toStylePrepareTimeoutError(name, url, timeoutMs) {
    return new Error(`Extension "${name}" stylesheet preprocessing timed out after ${timeoutMs}ms: ${url}`);
}

function toScriptLoadError(name, url, error) {
    if (error instanceof Error) {
        return error;
    }

    const errorType = error?.type || 'error';
    return new Error(`Extension "${name}" script load failed (${errorType}): ${url}`);
}

function toScriptTimeoutError(name, url, timeoutMs) {
    return new Error(`Extension "${name}" script load timed out after ${timeoutMs}ms: ${url}`);
}

function toScriptPrepareTimeoutError(name, url, timeoutMs) {
    return new Error(`Extension "${name}" script preprocessing timed out after ${timeoutMs}ms: ${url}`);
}

export function createExtensionAssetLoader({
    sanitizeSelector,
    getExtensionResourceUrl,
    isThirdPartyExtension,
    resolveThirdPartyModuleBlobUrl,
    resolveThirdPartyStylesheetBlobUrl,
    scriptLoadTimeoutMs = 30000,
    styleLoadTimeoutMs = 15000,
}) {
    async function addExtensionStyle(name, manifest) {
        if (!manifest.css) {
            return;
        }

        const id = sanitizeSelector(`${name}-css`);
        const existing = document.getElementById(id);
        if (existing) {
            if (existing.dataset.tauritavernLoaded === 'true') {
                return;
            }

            existing.remove();
        }

        let styleUrl = getExtensionResourceUrl(name, manifest.css);
        if (isThirdPartyExtension(name)) {
            styleUrl = await withTimeout(
                () => resolveThirdPartyStylesheetBlobUrl(styleUrl),
                styleLoadTimeoutMs,
                () => toStylePrepareTimeoutError(name, styleUrl, styleLoadTimeoutMs),
            );
        }

        await new Promise((resolve, reject) => {
            let settled = false;
            const link = document.createElement('link');
            const timeoutId = setTimeout(() => {
                if (settled) {
                    return;
                }
                settled = true;
                link.dataset.tauritavernLoaded = 'false';
                reject(toStyleTimeoutError(name, styleUrl, styleLoadTimeoutMs));
            }, styleLoadTimeoutMs);

            link.id = id;
            link.rel = 'stylesheet';
            link.type = 'text/css';
            link.href = styleUrl;
            link.onload = function () {
                if (settled) {
                    return;
                }
                settled = true;
                clearTimeout(timeoutId);
                link.dataset.tauritavernLoaded = 'true';
                resolve();
            };
            link.onerror = function (err) {
                if (settled) {
                    return;
                }
                settled = true;
                clearTimeout(timeoutId);
                link.dataset.tauritavernLoaded = 'false';
                reject(toStyleLoadError(name, styleUrl, err));
            };
            document.head.appendChild(link);
        });
    }

    async function addExtensionScript(name, manifest) {
        if (!manifest.js) {
            return;
        }

        const id = sanitizeSelector(`${name}-js`);
        const existing = document.getElementById(id);
        if (existing) {
            if (existing.dataset.tauritavernLoaded === 'true') {
                return;
            }

            existing.remove();
        }

        let scriptUrl = getExtensionResourceUrl(name, manifest.js);
        if (isThirdPartyExtension(name)) {
            scriptUrl = await withTimeout(
                () => resolveThirdPartyModuleBlobUrl(scriptUrl),
                scriptLoadTimeoutMs,
                () => toScriptPrepareTimeoutError(name, scriptUrl, scriptLoadTimeoutMs),
            );
        }

        await new Promise((resolve, reject) => {
            let settled = false;
            const script = document.createElement('script');
            const timeoutId = setTimeout(() => {
                if (settled) {
                    return;
                }
                settled = true;
                script.dataset.tauritavernLoaded = 'false';
                reject(toScriptTimeoutError(name, scriptUrl, scriptLoadTimeoutMs));
            }, scriptLoadTimeoutMs);

            script.id = id;
            script.type = 'module';
            script.src = scriptUrl;
            script.async = true;
            script.onerror = function (err) {
                if (settled) {
                    return;
                }
                settled = true;
                clearTimeout(timeoutId);
                script.dataset.tauritavernLoaded = 'false';
                reject(toScriptLoadError(name, scriptUrl, err));
            };
            script.onload = function () {
                if (settled) {
                    return;
                }
                settled = true;
                clearTimeout(timeoutId);
                script.dataset.tauritavernLoaded = 'true';
                resolve();
            };
            document.body.appendChild(script);
        });
    }

    return {
        addExtensionStyle,
        addExtensionScript,
    };
}
