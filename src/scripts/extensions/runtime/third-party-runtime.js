import { moduleLexerInit, moduleLexerParse, css as cssTools } from '../../../lib.js';

let moduleLexerReadyPromise = null;
let cssLayerSupportCache = null;
const THIRD_PARTY_ROUTE_URL_PATTERN = /(?:https?:\/\/[^\s"'`<>()]+)?\/scripts\/extensions\/third-party\/[^\s"'`<>()]+/g;
const MODULE_FILE_PATTERN = /\.m?js(?:[?#].*)?$/i;
const STYLESHEET_FILE_PATTERN = /\.css(?:[?#].*)?$/i;

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

function looksLikeHtmlBytes(bytes) {
    if (!(bytes instanceof Uint8Array) || bytes.length === 0) {
        return false;
    }

    try {
        const previewLength = Math.min(bytes.length, 256);
        const preview = new TextDecoder().decode(bytes.subarray(0, previewLength));
        return looksLikeHtmlPayload(preview);
    } catch {
        return false;
    }
}

function isOffsetWithinRanges(offset, ranges) {
    for (const range of ranges) {
        if (offset < range.start) {
            return false;
        }

        if (offset >= range.start && offset < range.end) {
            return true;
        }
    }

    return false;
}

function collectImportSpecifierRanges(source, imports) {
    const importRecords = Array.isArray(imports) ? imports : [];
    return importRecords
        .map(importRecord => getImportSpecifierReplaceRange(source, importRecord))
        .filter(range => Number.isInteger(range.start) && Number.isInteger(range.end) && range.start < range.end)
        .sort((left, right) => left.start - right.start);
}

function supportsCssCascadeLayers() {
    if (cssLayerSupportCache !== null) {
        return cssLayerSupportCache;
    }

    if (typeof window === 'undefined' || typeof document === 'undefined') {
        cssLayerSupportCache = true;
        return cssLayerSupportCache;
    }

    if (typeof window.CSSLayerBlockRule !== 'undefined' || typeof window.CSSLayerStatementRule !== 'undefined') {
        cssLayerSupportCache = true;
        return cssLayerSupportCache;
    }

    try {
        const style = document.createElement('style');
        style.textContent = '@layer tauritavern_probe { .tauritavern_probe { display: block; } }';
        (document.head || document.documentElement).appendChild(style);

        const stylesheet = style.sheet;
        const firstRule = stylesheet?.cssRules?.item?.(0) || stylesheet?.cssRules?.[0];
        const cssText = String(firstRule?.cssText || '').toLowerCase();
        cssLayerSupportCache = cssText.includes('@layer');

        style.remove();
    } catch {
        cssLayerSupportCache = false;
    }

    return cssLayerSupportCache;
}

function flattenLayerRules(rules) {
    const normalizedRules = Array.isArray(rules) ? rules : [];
    const flattened = [];

    for (const rule of normalizedRules) {
        if (!rule || typeof rule !== 'object') {
            continue;
        }

        if (rule.type === 'layer') {
            if (Array.isArray(rule.rules) && rule.rules.length > 0) {
                flattened.push(...flattenLayerRules(rule.rules));
            }
            continue;
        }

        if (Array.isArray(rule.rules) && rule.rules.length > 0) {
            rule.rules = flattenLayerRules(rule.rules);
        }

        flattened.push(rule);
    }

    return flattened;
}

function preprocessStylesheetForLegacyWebView(source) {
    const cssSource = String(source || '');
    if (!cssSource.includes('@layer') || supportsCssCascadeLayers()) {
        return cssSource;
    }

    if (!cssTools || typeof cssTools.parse !== 'function' || typeof cssTools.stringify !== 'function') {
        return cssSource;
    }

    try {
        const ast = cssTools.parse(cssSource, { silent: true });
        if (!ast?.stylesheet || !Array.isArray(ast.stylesheet.rules)) {
            return cssSource;
        }

        ast.stylesheet.rules = flattenLayerRules(ast.stylesheet.rules);
        return cssTools.stringify(ast, { compress: true });
    } catch {
        return cssSource;
    }
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
    const assetBlobCache = new Map();
    const assetBlobUrls = new Set();

    function cleanup() {
        for (const blobUrl of moduleBlobUrls) {
            URL.revokeObjectURL(blobUrl);
        }
        for (const blobUrl of styleBlobUrls) {
            URL.revokeObjectURL(blobUrl);
        }
        for (const blobUrl of assetBlobUrls) {
            URL.revokeObjectURL(blobUrl);
        }
        moduleBlobUrls.clear();
        moduleBlobCache.clear();
        styleBlobUrls.clear();
        styleBlobCache.clear();
        assetBlobUrls.clear();
        assetBlobCache.clear();
    }

    if (typeof window !== 'undefined') {
        window.addEventListener('beforeunload', cleanup, { once: true });
    }

    async function rewriteThirdPartyModuleSource(source, moduleUrl, processingChain = new Set()) {
        await ensureModuleLexerReady();
        const sourceText = String(source);
        const [imports] = moduleLexerParse(sourceText);
        const chain = new Set(processingChain);
        chain.add(normalizeThirdPartyModuleUrl(moduleUrl));
        let rewritten = sourceText;

        if (Array.isArray(imports) && imports.length > 0) {
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
        }

        return rewriteEmbeddedThirdPartyAssetUrls(rewritten, moduleUrl, chain);
    }

    function resolveThirdPartyInlineAssetUrl(routeUrl, processingChain = new Set()) {
        const normalized = normalizeThirdPartyModuleUrl(routeUrl);
        if (processingChain.has(normalized)) {
            return Promise.resolve(toBrowserModuleSpecifier(normalized));
        }

        const pathname = new URL(normalized, window.location.origin).pathname;
        if (MODULE_FILE_PATTERN.test(pathname)) {
            return resolveModuleBlobUrl(normalized, processingChain);
        }

        if (STYLESHEET_FILE_PATTERN.test(pathname)) {
            return resolveStylesheetBlobUrl(normalized);
        }

        return resolveAssetBlobUrl(normalized);
    }

    async function rewriteEmbeddedThirdPartyAssetUrls(source, moduleUrl, processingChain = new Set()) {
        const sourceText = String(source || '');
        if (!sourceText.includes('/scripts/extensions/third-party/')) {
            return sourceText;
        }

        let imports = [];
        try {
            [imports] = moduleLexerParse(sourceText);
        } catch {
            return sourceText;
        }
        const importSpecifierRanges = collectImportSpecifierRanges(sourceText, imports);
        const routeUrlPattern = new RegExp(THIRD_PARTY_ROUTE_URL_PATTERN.source, THIRD_PARTY_ROUTE_URL_PATTERN.flags);

        const matches = [];
        for (const match of sourceText.matchAll(routeUrlPattern)) {
            const candidate = String(match?.[0] || '');
            const start = Number(match?.index);
            if (!candidate || !Number.isInteger(start) || isOffsetWithinRanges(start, importSpecifierRanges)) {
                continue;
            }

            const resolved = resolveModuleSpecifier(moduleUrl, candidate);
            if (!resolved || !isRouteThirdPartyModuleUrl(resolved)) {
                continue;
            }

            matches.push({
                start,
                end: start + candidate.length,
                resolved: normalizeThirdPartyModuleUrl(resolved),
            });
        }

        if (matches.length === 0) {
            return sourceText;
        }

        const resolvedMap = new Map();
        for (const match of matches) {
            if (resolvedMap.has(match.resolved)) {
                continue;
            }

            const rewrittenUrl = await resolveThirdPartyInlineAssetUrl(match.resolved, processingChain);
            resolvedMap.set(match.resolved, rewrittenUrl);
        }

        let cursor = 0;
        let rewritten = '';
        for (const match of matches) {
            const replacement = resolvedMap.get(match.resolved);
            if (!replacement) {
                continue;
            }

            rewritten += sourceText.slice(cursor, match.start);
            rewritten += replacement;
            cursor = match.end;
        }
        rewritten += sourceText.slice(cursor);
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

    async function resolveAssetBlobUrl(assetUrl) {
        const doFetch = resolveFetchImpl(fetchImpl);
        const normalizedUrl = normalizeThirdPartyModuleUrl(assetUrl);
        const cachedTask = assetBlobCache.get(normalizedUrl);
        if (cachedTask) {
            return cachedTask;
        }

        const task = (async () => {
            const response = await doFetch(normalizedUrl, { cache: 'no-store' });
            if (!response.ok) {
                throw new Error(`Failed to fetch extension asset: ${response.status} ${response.statusText}`);
            }

            const contentType = String(response.headers.get('content-type') || '').toLowerCase();
            const bytes = new Uint8Array(await response.arrayBuffer());
            if (contentType.includes('text/html') || looksLikeHtmlBytes(bytes)) {
                throw new Error(`Extension asset is not binary/text payload: ${normalizedUrl}`);
            }

            const blobUrl = URL.createObjectURL(new Blob([bytes], { type: contentType || 'application/octet-stream' }));
            assetBlobUrls.add(blobUrl);
            return blobUrl;
        })();

        assetBlobCache.set(normalizedUrl, task);

        try {
            return await task;
        } catch (error) {
            assetBlobCache.delete(normalizedUrl);
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

            const preparedStylesheet = preprocessStylesheetForLegacyWebView(source);
            const blobUrl = URL.createObjectURL(new Blob([preparedStylesheet], { type: 'text/css' }));
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
