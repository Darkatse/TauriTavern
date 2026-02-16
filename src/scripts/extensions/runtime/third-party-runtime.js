import { moduleLexerInit, moduleLexerParse } from '../../../lib.js';

let moduleLexerReadyPromise = null;

function ensureModuleLexerReady() {
    if (!moduleLexerReadyPromise) {
        moduleLexerReadyPromise = Promise.resolve(moduleLexerInit);
    }

    return moduleLexerReadyPromise;
}

function normalizeThirdPartyModuleUrl(url) {
    const parsed = new URL(String(url), window.location.origin);
    parsed.hash = '';
    return parsed.href;
}

function isRouteThirdPartyModuleUrl(url) {
    const parsed = new URL(String(url), window.location.origin);
    return parsed.origin === window.location.origin
        && parsed.pathname.startsWith('/scripts/extensions/third-party/');
}

function resolveModuleSpecifier(baseUrl, specifier) {
    if (typeof specifier !== 'string' || !specifier) {
        return null;
    }

    const isRelativeSpecifier = specifier.startsWith('./') || specifier.startsWith('../');
    const isRootSpecifier = specifier.startsWith('/');
    const hasProtocol = /^[a-zA-Z][a-zA-Z0-9+\-.]*:/.test(specifier);

    if (!isRelativeSpecifier && !isRootSpecifier && !hasProtocol) {
        return null;
    }

    try {
        return new URL(specifier, baseUrl).href;
    } catch {
        return null;
    }
}

function toBrowserModuleSpecifier(url) {
    const parsed = new URL(String(url), window.location.origin);
    return parsed.href;
}

function isQuotedSpecifierSlice(value) {
    if (!value || value.length < 2) {
        return false;
    }

    const quote = value[0];
    return (quote === '\'' || quote === '"' || quote === '`') && value[value.length - 1] === quote;
}

function getImportSpecifierReplaceRange(source, importRecord) {
    let start = importRecord.s;
    let end = importRecord.e;
    const before = source[start - 1];
    const after = source[end];
    if ((before === '\'' || before === '"' || before === '`') && after === before) {
        start -= 1;
        end += 1;
    }

    return { start, end };
}

function isDynamicImportRecord(importRecord) {
    return importRecord?.t === 2 || Number(importRecord?.d) >= 0;
}

function looksLikeHtmlPayload(text) {
    return /^\s*</.test(String(text || ''));
}

function resolveFetchImpl(fetchImpl) {
    if (typeof fetchImpl === 'function') {
        return fetchImpl;
    }

    if (typeof window !== 'undefined' && typeof window.fetch === 'function') {
        return window.fetch.bind(window);
    }

    if (typeof globalThis.fetch === 'function') {
        return globalThis.fetch.bind(globalThis);
    }

    throw new Error('fetch is unavailable');
}

export function createThirdPartyBlobResolver({ fetchImpl } = {}) {
    const moduleBlobCache = new Map();
    const moduleBlobUrls = new Set();
    const styleBlobCache = new Map();
    const styleBlobUrls = new Set();

    function cleanup() {
        for (const blobUrl of moduleBlobUrls) {
            URL.revokeObjectURL(blobUrl);
        }
        for (const blobUrl of styleBlobUrls) {
            URL.revokeObjectURL(blobUrl);
        }
        moduleBlobUrls.clear();
        moduleBlobCache.clear();
        styleBlobUrls.clear();
        styleBlobCache.clear();
    }

    if (typeof window !== 'undefined') {
        window.addEventListener('beforeunload', cleanup, { once: true });
    }

    async function rewriteThirdPartyModuleSource(source, moduleUrl, processingChain = new Set()) {
        await ensureModuleLexerReady();
        const [imports] = moduleLexerParse(String(source));
        if (!Array.isArray(imports) || imports.length === 0) {
            return String(source);
        }

        const chain = new Set(processingChain);
        chain.add(normalizeThirdPartyModuleUrl(moduleUrl));
        let rewritten = String(source);
        for (let index = imports.length - 1; index >= 0; index -= 1) {
            const importRecord = imports[index];
            if (typeof importRecord?.n !== 'string') {
                continue;
            }

            const resolved = resolveModuleSpecifier(moduleUrl, importRecord.n);
            if (!resolved) {
                continue;
            }

            let replacement = importRecord.n;
            if (isRouteThirdPartyModuleUrl(resolved)) {
                const normalizedResolved = normalizeThirdPartyModuleUrl(resolved);
                const isCircularDependency = chain.has(normalizedResolved);
                const isDynamicImport = isDynamicImportRecord(importRecord);

                replacement = (isCircularDependency || isDynamicImport)
                    ? toBrowserModuleSpecifier(resolved)
                    : await resolveModuleBlobUrl(resolved, chain);
            } else {
                replacement = toBrowserModuleSpecifier(resolved);
            }

            if (replacement !== importRecord.n) {
                const { start, end } = getImportSpecifierReplaceRange(rewritten, importRecord);
                const currentSlice = rewritten.slice(start, end);
                const replacementLiteral = isQuotedSpecifierSlice(currentSlice)
                    ? JSON.stringify(replacement)
                    : replacement;
                rewritten = `${rewritten.slice(0, start)}${replacementLiteral}${rewritten.slice(end)}`;
            }
        }

        return rewritten;
    }

    async function resolveModuleBlobUrl(moduleUrl, processingChain = new Set()) {
        const doFetch = resolveFetchImpl(fetchImpl);
        const normalizedUrl = normalizeThirdPartyModuleUrl(moduleUrl);
        const cachedTask = moduleBlobCache.get(normalizedUrl);
        if (cachedTask) {
            return cachedTask;
        }

        const chain = new Set(processingChain);
        chain.add(normalizedUrl);
        const task = (async () => {
            const response = await doFetch(normalizedUrl, { cache: 'no-store' });
            if (!response.ok) {
                throw new Error(`Failed to fetch extension module: ${response.status} ${response.statusText}`);
            }

            const contentType = String(response.headers.get('content-type') || '').toLowerCase();
            const source = await response.text();
            if (contentType.includes('text/html') || looksLikeHtmlPayload(source)) {
                throw new Error(`Extension module is not JavaScript: ${normalizedUrl}`);
            }

            const rewritten = await rewriteThirdPartyModuleSource(source, normalizedUrl, chain);
            const blobUrl = URL.createObjectURL(new Blob([rewritten], { type: 'text/javascript' }));
            moduleBlobUrls.add(blobUrl);
            return blobUrl;
        })();

        moduleBlobCache.set(normalizedUrl, task);

        try {
            return await task;
        } catch (error) {
            moduleBlobCache.delete(normalizedUrl);
            throw error;
        }
    }

    async function resolveStylesheetBlobUrl(stylesheetUrl) {
        const doFetch = resolveFetchImpl(fetchImpl);
        const normalizedUrl = normalizeThirdPartyModuleUrl(stylesheetUrl);
        const cachedTask = styleBlobCache.get(normalizedUrl);
        if (cachedTask) {
            return cachedTask;
        }

        const task = (async () => {
            const response = await doFetch(normalizedUrl, { cache: 'no-store' });
            if (!response.ok) {
                throw new Error(`Failed to fetch extension stylesheet: ${response.status} ${response.statusText}`);
            }

            const contentType = String(response.headers.get('content-type') || '').toLowerCase();
            const source = await response.text();
            if (contentType.includes('text/html') || looksLikeHtmlPayload(source)) {
                throw new Error(`Extension stylesheet is not CSS: ${normalizedUrl}`);
            }

            const blobUrl = URL.createObjectURL(new Blob([source], { type: 'text/css' }));
            styleBlobUrls.add(blobUrl);
            return blobUrl;
        })();

        styleBlobCache.set(normalizedUrl, task);

        try {
            return await task;
        } catch (error) {
            styleBlobCache.delete(normalizedUrl);
            throw error;
        }
    }

    return {
        resolveModuleBlobUrl,
        resolveStylesheetBlobUrl,
        cleanup,
    };
}
