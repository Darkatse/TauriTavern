import { css as cssTools } from '../../../lib.js';

let cssLayerSupportCache = null;

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

function isCssUrlBypassScheme(url) {
    const normalized = String(url || '').trim().toLowerCase();
    return normalized.startsWith('data:')
        || normalized.startsWith('blob:')
        || normalized.startsWith('about:')
        || normalized.startsWith('javascript:')
        || normalized.startsWith('#');
}

function absolutizeStylesheetUrls(source, stylesheetUrl) {
    const cssSource = String(source || '');
    if (!cssSource.includes('url(')) {
        return cssSource;
    }

    return cssSource.replace(/url\(\s*(['"]?)([^'")]+)\1\s*\)/gi, (fullMatch, quote, rawUrl) => {
        const candidate = String(rawUrl || '').trim();
        if (!candidate || isCssUrlBypassScheme(candidate)) {
            return fullMatch;
        }

        try {
            const absoluteUrl = new URL(candidate, stylesheetUrl).href;
            const wrappedQuote = quote || '"';
            return `url(${wrappedQuote}${absoluteUrl}${wrappedQuote})`;
        } catch {
            return fullMatch;
        }
    });
}

function absolutizeStylesheetImports(source, stylesheetUrl) {
    const cssSource = String(source || '');
    if (!/@import/i.test(cssSource)) {
        return cssSource;
    }

    return cssSource.replace(/(@import\s+)(['"])([^'"]+)\2/gi, (fullMatch, prefix, quote, rawUrl) => {
        const candidate = String(rawUrl || '').trim();
        if (!candidate || isCssUrlBypassScheme(candidate)) {
            return fullMatch;
        }

        try {
            const absoluteUrl = new URL(candidate, stylesheetUrl).href;
            return `${prefix}${quote}${absoluteUrl}${quote}`;
        } catch {
            return fullMatch;
        }
    });
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

function normalizeStylesheetUrl(stylesheetUrl) {
    const parsed = new URL(String(stylesheetUrl), window.location.origin);
    parsed.hash = '';
    return parsed.href;
}

export function createThirdPartyStylesheetResolver({ fetchImpl } = {}) {
    const styleBlobCache = new Map();
    const styleBlobUrls = new Set();

    function cleanup() {
        for (const blobUrl of styleBlobUrls) {
            URL.revokeObjectURL(blobUrl);
        }
        styleBlobUrls.clear();
        styleBlobCache.clear();
    }

    if (typeof window !== 'undefined') {
        window.addEventListener('beforeunload', cleanup, { once: true });
    }

    async function resolveStylesheetUrl(stylesheetUrl) {
        const normalizedUrl = normalizeStylesheetUrl(stylesheetUrl);
        if (supportsCssCascadeLayers()) {
            return normalizedUrl;
        }

        const cachedTask = styleBlobCache.get(normalizedUrl);
        if (cachedTask) {
            return cachedTask;
        }

        const task = (async () => {
            const doFetch = resolveFetchImpl(fetchImpl);
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
            const rewrittenImports = absolutizeStylesheetImports(preparedStylesheet, normalizedUrl);
            const absolutizedStylesheet = absolutizeStylesheetUrls(rewrittenImports, normalizedUrl);
            const blobUrl = URL.createObjectURL(new Blob([absolutizedStylesheet], { type: 'text/css' }));
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
        resolveStylesheetUrl,
        cleanup,
    };
}
