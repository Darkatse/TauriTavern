import { moduleLexerInit, moduleLexerParse, css as cssTools } from '../../../lib.js';

let moduleLexerReadyPromise = null;
let cssLayerSupportCache = null;
const THIRD_PARTY_ROUTE_URL_PATTERN = /(?:https?:\/\/[^\s"'`<>()]+)?\/scripts\/extensions\/third-party\/[^\s"'`<>()]+/g;
const MODULE_FILE_PATTERN = /\.m?js(?:[?#].*)?$/i;
const STYLESHEET_FILE_PATTERN = /\.css(?:[?#].*)?$/i;
const DOCUMENT_FILE_PATTERN = /\.html?(?:[?#].*)?$/i;
const DOM_PATCH_PENDING_ATTR = 'data-tauritavern-pending-url';
const DOM_PATCH_INTERNAL_FLAG = Symbol('tauritavern.thirdParty.domPatch');
const DOM_PATCH_QUERY_SELECTOR = 'script[src*="scripts/extensions/third-party/"],link[href*="scripts/extensions/third-party/"],iframe[src*="scripts/extensions/third-party/"]';
const THIRD_PARTY_IMPORT_BRIDGE_KEY = '__TAURITAVERN_THIRD_PARTY_IMPORT_MODULE__';

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

function looksLikeHtmlPayload(text) {
    return /^\s*</.test(String(text || ''));
}

function normalizeModuleSource(source) {
    return String(source || '').replace(/^\uFEFF+/, '');
}

function resolveRouteThirdPartyUrl(url, baseUrl = window.location.origin) {
    if (typeof url !== 'string' || !url.trim()) {
        return null;
    }

    try {
        const resolved = new URL(url, baseUrl).href;
        return isRouteThirdPartyModuleUrl(resolved)
            ? normalizeThirdPartyModuleUrl(resolved)
            : null;
    } catch {
        return null;
    }
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

function collectModuleExportNames(exportRecords) {
    if (!Array.isArray(exportRecords) || exportRecords.length === 0) {
        return [];
    }

    const names = [];
    for (const record of exportRecords) {
        if (typeof record === 'string') {
            if (record) {
                names.push(record);
            }
            continue;
        }

        if (!record || typeof record !== 'object') {
            continue;
        }

        const primaryName = typeof record.n === 'string' ? record.n : '';
        const localName = typeof record.ln === 'string' ? record.ln : '';
        const candidate = primaryName || localName;
        if (candidate) {
            names.push(candidate);
        }
    }

    return [...new Set(names)];
}

function buildCircularModuleShimSource(routeUrl, exportNames) {
    const uniqueNames = [...new Set(
        (Array.isArray(exportNames) ? exportNames : [])
            .map(name => String(name || '').trim())
            .filter(Boolean),
    )];
    const nonDefaultNames = uniqueNames.filter(name => name !== 'default');
    const hasDefaultExport = uniqueNames.includes('default');
    const namedExports = nonDefaultNames.map((name, index) => ({
        name,
        local: `__tt_export_${index}`,
    }));

    const lines = [
        `const __ttImportBridge = globalThis[${JSON.stringify(THIRD_PARTY_IMPORT_BRIDGE_KEY)}];`,
        'if (typeof __ttImportBridge !== "function") {',
        '    throw new Error("[TauriTavern] Third-party import bridge is unavailable");',
        '}',
    ];

    for (const binding of namedExports) {
        lines.push(`let ${binding.local};`);
    }
    if (hasDefaultExport) {
        lines.push('let __tt_default_export;');
    }

    const exportSpecifiers = namedExports.map(binding => `${binding.local} as ${binding.name}`);
    if (hasDefaultExport) {
        exportSpecifiers.push('__tt_default_export as default');
    }
    if (exportSpecifiers.length > 0) {
        lines.push(`export { ${exportSpecifiers.join(', ')} };`);
    }

    lines.push(`Promise.resolve(__ttImportBridge(${JSON.stringify(routeUrl)}))`);
    lines.push('    .then((__ttNamespace) => {');
    for (const binding of namedExports) {
        lines.push(`        ${binding.local} = __ttNamespace?.[${JSON.stringify(binding.name)}];`);
    }
    if (hasDefaultExport) {
        lines.push('        __tt_default_export = __ttNamespace?.default;');
    }
    lines.push('    })');
    lines.push('    .catch((error) => {');
    lines.push(`        console.error("[TauriTavern] Failed to hydrate circular module shim:", ${JSON.stringify(routeUrl)}, error);`);
    lines.push('    });');

    return lines.join('\n');
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
    const moduleNamespaceCache = new Map();
    const moduleCircularShimCache = new Map();
    const moduleBlobUrls = new Set();
    const stylesheetSourceCache = new Map();
    const styleBlobCache = new Map();
    const styleBlobUrls = new Set();
    const documentBlobCache = new Map();
    const documentBlobUrls = new Set();
    const assetBlobCache = new Map();
    const assetBlobUrls = new Set();
    let domAssetPatchInstalled = false;

    function cleanup() {
        for (const blobUrl of moduleBlobUrls) {
            URL.revokeObjectURL(blobUrl);
        }
        for (const blobUrl of styleBlobUrls) {
            URL.revokeObjectURL(blobUrl);
        }
        for (const blobUrl of documentBlobUrls) {
            URL.revokeObjectURL(blobUrl);
        }
        for (const blobUrl of assetBlobUrls) {
            URL.revokeObjectURL(blobUrl);
        }
        moduleBlobUrls.clear();
        moduleBlobCache.clear();
        moduleNamespaceCache.clear();
        moduleCircularShimCache.clear();
        stylesheetSourceCache.clear();
        styleBlobUrls.clear();
        styleBlobCache.clear();
        documentBlobUrls.clear();
        documentBlobCache.clear();
        assetBlobUrls.clear();
        assetBlobCache.clear();
    }

    if (typeof window !== 'undefined') {
        window.addEventListener('beforeunload', cleanup, { once: true });
    }

    function inferThirdPartyAssetKindByPath(routeUrl) {
        const pathname = new URL(routeUrl, window.location.origin).pathname;
        if (MODULE_FILE_PATTERN.test(pathname)) {
            return 'module';
        }

        if (STYLESHEET_FILE_PATTERN.test(pathname)) {
            return 'style';
        }

        if (DOCUMENT_FILE_PATTERN.test(pathname)) {
            return 'document';
        }

        return 'asset';
    }

    function resolveThirdPartyOrExternalUrl(url, baseUrl = window.location.origin) {
        const thirdPartyRouteUrl = resolveRouteThirdPartyUrl(url, baseUrl);
        if (thirdPartyRouteUrl) {
            return {
                type: 'third-party',
                url: thirdPartyRouteUrl,
            };
        }

        if (typeof url !== 'string' || !url.trim()) {
            return null;
        }

        try {
            const parsed = new URL(url, baseUrl);
            if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
                return null;
            }

            parsed.hash = '';
            return {
                type: 'external',
                url: parsed.href,
            };
        } catch {
            return null;
        }
    }

    function isModuleScriptElement(node) {
        const type = String(node?.getAttribute?.('type') || node?.type || '').trim().toLowerCase();
        return type === 'module';
    }

    function readPendingDomAssetCandidate(node) {
        if (!node || typeof window === 'undefined') {
            return null;
        }

        if (node instanceof window.HTMLScriptElement) {
            const candidate = node.getAttribute('src') || node.getAttribute(DOM_PATCH_PENDING_ATTR) || node.src;
            if (!candidate) {
                return null;
            }

            return {
                kind: isModuleScriptElement(node) ? 'module' : 'script',
                url: candidate,
                attribute: 'src',
            };
        }

        if (node instanceof window.HTMLLinkElement) {
            const rel = String(node.getAttribute('rel') || node.rel || '').toLowerCase();
            if (rel !== 'stylesheet' && rel !== 'modulepreload') {
                return null;
            }

            const candidate = node.getAttribute('href') || node.getAttribute(DOM_PATCH_PENDING_ATTR) || node.href;
            if (!candidate) {
                return null;
            }

            return {
                kind: rel === 'modulepreload' ? 'module' : 'style',
                url: candidate,
                attribute: 'href',
            };
        }

        if (node instanceof window.HTMLIFrameElement) {
            const candidate = node.getAttribute('src') || node.getAttribute(DOM_PATCH_PENDING_ATTR) || node.src;
            if (!candidate) {
                return null;
            }

            return {
                kind: 'document',
                url: candidate,
                attribute: 'src',
            };
        }

        return null;
    }

    function collectPendingDomAssetRequests(node) {
        if (!node || typeof window === 'undefined') {
            return [];
        }

        const requests = [];
        const visitedNodes = new Set();
        const tryCollect = (candidateNode) => {
            if (!candidateNode || visitedNodes.has(candidateNode)) {
                return;
            }
            visitedNodes.add(candidateNode);

            const request = readPendingDomAssetCandidate(candidateNode);
            if (!request) {
                return;
            }

            const routeUrl = resolveRouteThirdPartyUrl(request.url);
            if (!routeUrl) {
                return;
            }

            requests.push({ node: candidateNode, request, routeUrl });
        };

        tryCollect(node);

        if (typeof node.querySelectorAll === 'function') {
            for (const candidateNode of node.querySelectorAll(DOM_PATCH_QUERY_SELECTOR)) {
                tryCollect(candidateNode);
            }
        }

        return requests;
    }

    function markNodeForDeferredAssetLoad(node, request) {
        if (!node || node[DOM_PATCH_INTERNAL_FLAG]) {
            return false;
        }

        node[DOM_PATCH_INTERNAL_FLAG] = true;
        node.setAttribute(DOM_PATCH_PENDING_ATTR, request.url);
        try {
            node.removeAttribute(request.attribute);
        } catch {
            // Ignore attribute removal failure.
        }

        return true;
    }

    async function resolveAndApplyDomAsset(node, request, routeUrl) {
        try {
            const resolvedUrl = await resolveThirdPartyInlineAssetUrl(routeUrl, new Set(), request.kind);
            if (!resolvedUrl) {
                return;
            }

            node.setAttribute(request.attribute, resolvedUrl);
            node.removeAttribute(DOM_PATCH_PENDING_ATTR);
        } catch (error) {
            console.error('[TauriTavern] Failed to resolve third-party DOM asset:', routeUrl, error);
            node.dispatchEvent(new Event('error'));
        } finally {
            delete node[DOM_PATCH_INTERNAL_FLAG];
        }
    }

    function maybeScheduleDomAssetRewrite(node) {
        const requests = collectPendingDomAssetRequests(node);
        if (requests.length === 0) {
            return;
        }

        for (const pending of requests) {
            const isMarked = markNodeForDeferredAssetLoad(pending.node, pending.request);
            if (!isMarked) {
                continue;
            }

            queueMicrotask(() => {
                void resolveAndApplyDomAsset(pending.node, pending.request, pending.routeUrl);
            });
        }
    }

    function installDomAssetPatch() {
        if (domAssetPatchInstalled || typeof window === 'undefined') {
            return;
        }

        const elementProto = window.Element?.prototype;
        if (!elementProto) {
            return;
        }

        const originalAppendChild = elementProto.appendChild;
        const originalInsertBefore = elementProto.insertBefore;

        elementProto.appendChild = function patchedAppendChild(child) {
            maybeScheduleDomAssetRewrite(child);
            return originalAppendChild.call(this, child);
        };

        elementProto.insertBefore = function patchedInsertBefore(newNode, referenceNode) {
            maybeScheduleDomAssetRewrite(newNode);
            return originalInsertBefore.call(this, newNode, referenceNode);
        };

        const iframePrototype = window.HTMLIFrameElement?.prototype;
        const iframeSrcDescriptor = iframePrototype
            ? Object.getOwnPropertyDescriptor(iframePrototype, 'src')
            : null;
        if (iframeSrcDescriptor && typeof iframeSrcDescriptor.set === 'function' && typeof iframeSrcDescriptor.get === 'function') {
            Object.defineProperty(iframePrototype, 'src', {
                configurable: true,
                enumerable: iframeSrcDescriptor.enumerable,
                get() {
                    return iframeSrcDescriptor.get.call(this);
                },
                set(value) {
                    if (this[DOM_PATCH_INTERNAL_FLAG]) {
                        iframeSrcDescriptor.set.call(this, value);
                        return;
                    }

                    const routeUrl = resolveRouteThirdPartyUrl(value);
                    if (!routeUrl) {
                        iframeSrcDescriptor.set.call(this, value);
                        return;
                    }

                    const request = {
                        kind: 'document',
                        url: String(value),
                        attribute: 'src',
                    };
                    const isMarked = markNodeForDeferredAssetLoad(this, request);
                    if (!isMarked) {
                        iframeSrcDescriptor.set.call(this, value);
                        return;
                    }

                    queueMicrotask(() => {
                        void resolveAndApplyDomAsset(this, request, routeUrl);
                    });
                },
            });
        }

        const originalOpen = typeof window.open === 'function'
            ? window.open.bind(window)
            : null;
        if (originalOpen) {
            window.open = function patchedWindowOpen(url, target, features) {
                const routeUrl = resolveRouteThirdPartyUrl(url);
                if (!routeUrl) {
                    return originalOpen(url, target, features);
                }

                const fallbackUrl = toBrowserModuleSpecifier(routeUrl);
                const popup = originalOpen('about:blank', target, features);
                if (!popup) {
                    return originalOpen(fallbackUrl, target, features);
                }

                const preferredKind = inferThirdPartyAssetKindByPath(routeUrl);
                queueMicrotask(() => {
                    void resolveThirdPartyInlineAssetUrl(routeUrl, new Set(), preferredKind)
                        .then((resolvedUrl) => {
                            if (!popup || popup.closed) {
                                return;
                            }

                            const targetUrl = resolvedUrl || fallbackUrl;
                            try {
                                popup.location.replace(targetUrl);
                            } catch {
                                // Ignore popup navigation failures.
                            }
                        })
                        .catch((error) => {
                            console.error('[TauriTavern] Failed to resolve third-party popup asset:', routeUrl, error);
                            if (!popup || popup.closed) {
                                return;
                            }

                            try {
                                popup.location.replace(fallbackUrl);
                            } catch {
                                // Ignore popup navigation failures.
                            }
                        });
                });

                return popup;
            };
        }

        domAssetPatchInstalled = true;
    }

    async function importThirdPartyModuleNamespace(routeUrl) {
        const normalizedUrl = normalizeThirdPartyModuleUrl(routeUrl);
        if (!isRouteThirdPartyModuleUrl(normalizedUrl)) {
            return import(toBrowserModuleSpecifier(normalizedUrl));
        }

        const cachedTask = moduleNamespaceCache.get(normalizedUrl);
        if (cachedTask) {
            return cachedTask;
        }

        const task = (async () => {
            const moduleBlobUrl = await resolveModuleBlobUrl(normalizedUrl, new Set());
            return import(moduleBlobUrl);
        })();
        moduleNamespaceCache.set(normalizedUrl, task);

        try {
            return await task;
        } catch (error) {
            moduleNamespaceCache.delete(normalizedUrl);
            throw error;
        }
    }

    function installThirdPartyImportBridge() {
        if (typeof globalThis === 'undefined') {
            return;
        }

        globalThis[THIRD_PARTY_IMPORT_BRIDGE_KEY] = importThirdPartyModuleNamespace;
    }

    async function resolveCircularModuleShimUrl(moduleUrl) {
        const doFetch = resolveFetchImpl(fetchImpl);
        const normalizedUrl = normalizeThirdPartyModuleUrl(moduleUrl);
        const cachedTask = moduleCircularShimCache.get(normalizedUrl);
        if (cachedTask) {
            return cachedTask;
        }

        const task = (async () => {
            const response = await doFetch(normalizedUrl, { cache: 'no-store' });
            if (!response.ok) {
                throw new Error(`Failed to fetch circular extension module: ${response.status} ${response.statusText}`);
            }

            const contentType = String(response.headers.get('content-type') || '').toLowerCase();
            const source = normalizeModuleSource(await response.text());
            if (contentType.includes('text/html') || looksLikeHtmlPayload(source)) {
                throw new Error(`Circular extension module is not JavaScript: ${normalizedUrl}`);
            }

            await ensureModuleLexerReady();
            const [, exportRecords] = moduleLexerParse(source);
            const exportNames = collectModuleExportNames(exportRecords);
            const shimSource = buildCircularModuleShimSource(normalizedUrl, exportNames);
            const shimBlobUrl = URL.createObjectURL(new Blob([shimSource], { type: 'text/javascript' }));
            moduleBlobUrls.add(shimBlobUrl);
            return shimBlobUrl;
        })();

        moduleCircularShimCache.set(normalizedUrl, task);
        try {
            return await task;
        } catch (error) {
            moduleCircularShimCache.delete(normalizedUrl);
            throw error;
        }
    }

    async function rewriteThirdPartyModuleSource(source, moduleUrl, processingChain = new Set()) {
        await ensureModuleLexerReady();
        const sourceText = normalizeModuleSource(source);
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

                    replacement = isCircularDependency
                        ? await resolveCircularModuleShimUrl(resolved)
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

    function resolveThirdPartyInlineAssetUrl(routeUrl, processingChain = new Set(), preferredKind = 'auto') {
        const normalized = normalizeThirdPartyModuleUrl(routeUrl);
        if (processingChain.has(normalized)) {
            return Promise.resolve(toBrowserModuleSpecifier(normalized));
        }

        if (preferredKind === 'module') {
            return resolveModuleBlobUrl(normalized, processingChain);
        }

        if (preferredKind === 'style') {
            return resolveStylesheetBlobUrl(normalized);
        }

        if (preferredKind === 'document') {
            return resolveDocumentBlobUrl(normalized, processingChain);
        }

        if (preferredKind === 'script' || preferredKind === 'asset') {
            return resolveAssetBlobUrl(normalized);
        }

        const pathname = new URL(normalized, window.location.origin).pathname;
        if (MODULE_FILE_PATTERN.test(pathname)) {
            return resolveModuleBlobUrl(normalized, processingChain);
        }

        if (STYLESHEET_FILE_PATTERN.test(pathname)) {
            return resolveStylesheetBlobUrl(normalized);
        }

        if (DOCUMENT_FILE_PATTERN.test(pathname)) {
            return resolveDocumentBlobUrl(normalized, processingChain);
        }

        return resolveAssetBlobUrl(normalized);
    }

    installThirdPartyImportBridge();
    installDomAssetPatch();

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
            if (candidate.includes('${')) {
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

    async function rewriteThirdPartyDocumentSource(source, documentUrl, processingChain = new Set()) {
        if (typeof DOMParser === 'undefined') {
            return source;
        }

        const sourceText = String(source || '');
        const parser = new DOMParser();
        const parsedDocument = parser.parseFromString(sourceText, 'text/html');
        const htmlElement = parsedDocument?.documentElement;
        if (!htmlElement) {
            return sourceText;
        }

        const headElement = parsedDocument.head || parsedDocument.createElement('head');
        if (!parsedDocument.head) {
            if (htmlElement.firstChild) {
                htmlElement.insertBefore(headElement, htmlElement.firstChild);
            } else {
                htmlElement.appendChild(headElement);
            }
        }

        let baseElement = headElement.querySelector('base');
        if (!baseElement) {
            baseElement = parsedDocument.createElement('base');
            if (headElement.firstChild) {
                headElement.insertBefore(baseElement, headElement.firstChild);
            } else {
                headElement.appendChild(baseElement);
            }
        }
        baseElement.setAttribute('href', documentUrl);

        const resolvedCache = new Map();
        const resolveDocumentAsset = async (node, attribute, preferredKind = 'auto') => {
            const candidate = node.getAttribute(attribute);
            if (!candidate) {
                return;
            }

            const resolvedTarget = resolveThirdPartyOrExternalUrl(candidate, documentUrl);
            if (!resolvedTarget) {
                return;
            }

            if (preferredKind === 'style' && resolvedTarget.type === 'third-party') {
                try {
                    const stylesheetSource = await loadPreparedStylesheetSource(resolvedTarget.url);
                    const styleElement = parsedDocument.createElement('style');
                    styleElement.setAttribute('data-tauritavern-inline-style', resolvedTarget.url);
                    styleElement.textContent = stylesheetSource;
                    node.replaceWith(styleElement);
                    return;
                } catch (error) {
                    console.warn('[TauriTavern] Failed to inline third-party stylesheet, falling back to blob URL:', resolvedTarget.url, error);
                }
            }

            const cacheKey = `${preferredKind}:${resolvedTarget.url}`;
            let resolvedUrlPromise = resolvedCache.get(cacheKey);
            if (!resolvedUrlPromise) {
                resolvedUrlPromise = resolveThirdPartyInlineAssetUrl(resolvedTarget.url, processingChain, preferredKind);
                resolvedCache.set(cacheKey, resolvedUrlPromise);
            }

            let resolvedUrl;
            try {
                resolvedUrl = await resolvedUrlPromise;
            } catch (error) {
                resolvedCache.delete(cacheKey);
                console.warn('[TauriTavern] Failed to proxy document asset, using original URL:', resolvedTarget.url, error);
                return;
            }
            if (resolvedUrl) {
                node.setAttribute(attribute, resolvedUrl);
            }
        };

        for (const scriptElement of parsedDocument.querySelectorAll('script[src]')) {
            const scriptKind = isModuleScriptElement(scriptElement) ? 'module' : 'script';
            await resolveDocumentAsset(scriptElement, 'src', scriptKind);
        }

        for (const linkElement of parsedDocument.querySelectorAll('link[href]')) {
            const rel = String(linkElement.getAttribute('rel') || '').toLowerCase();
            const preferredKind = rel === 'stylesheet'
                ? 'style'
                : rel === 'modulepreload'
                    ? 'module'
                    : 'asset';
            await resolveDocumentAsset(linkElement, 'href', preferredKind);
        }

        for (const iframeElement of parsedDocument.querySelectorAll('iframe[src]')) {
            await resolveDocumentAsset(iframeElement, 'src', 'document');
        }

        for (const mediaElement of parsedDocument.querySelectorAll('img[src],audio[src],video[src],source[src],track[src],embed[src],input[src]')) {
            await resolveDocumentAsset(mediaElement, 'src', 'asset');
        }

        for (const objectElement of parsedDocument.querySelectorAll('object[data]')) {
            await resolveDocumentAsset(objectElement, 'data', 'asset');
        }

        for (const sourceElement of parsedDocument.querySelectorAll('source[srcset],img[srcset]')) {
            const srcset = sourceElement.getAttribute('srcset');
            if (!srcset) {
                continue;
            }

            const segments = srcset
                .split(',')
                .map(segment => segment.trim())
                .filter(Boolean);
            if (segments.length === 0) {
                continue;
            }

            let hasChanged = false;
            const rewrittenSegments = [];
            for (const segment of segments) {
                const [urlToken, ...descriptorTokens] = segment.split(/\s+/);
                if (!urlToken) {
                    continue;
                }

                const resolvedTarget = resolveThirdPartyOrExternalUrl(urlToken, documentUrl);
                if (!resolvedTarget) {
                    rewrittenSegments.push(segment);
                    continue;
                }

                const cacheKey = `asset:${resolvedTarget.url}`;
                let resolvedUrlPromise = resolvedCache.get(cacheKey);
                if (!resolvedUrlPromise) {
                    resolvedUrlPromise = resolveThirdPartyInlineAssetUrl(resolvedTarget.url, processingChain, 'asset');
                    resolvedCache.set(cacheKey, resolvedUrlPromise);
                }

                let resolvedUrl;
                try {
                    resolvedUrl = await resolvedUrlPromise;
                } catch (error) {
                    resolvedCache.delete(cacheKey);
                    console.warn('[TauriTavern] Failed to proxy document srcset asset, using original URL:', resolvedTarget.url, error);
                    rewrittenSegments.push(segment);
                    continue;
                }

                if (!resolvedUrl) {
                    rewrittenSegments.push(segment);
                    continue;
                }

                hasChanged = true;
                const descriptor = descriptorTokens.join(' ').trim();
                rewrittenSegments.push(descriptor ? `${resolvedUrl} ${descriptor}` : resolvedUrl);
            }

            if (hasChanged && rewrittenSegments.length > 0) {
                sourceElement.setAttribute('srcset', rewrittenSegments.join(', '));
            }
        }

        const doctype = parsedDocument.doctype?.name
            ? `<!DOCTYPE ${parsedDocument.doctype.name}>`
            : '<!DOCTYPE html>';
        return `${doctype}\n${htmlElement.outerHTML}`;
    }

    async function resolveDocumentBlobUrl(documentUrl, processingChain = new Set()) {
        const doFetch = resolveFetchImpl(fetchImpl);
        const normalizedUrl = normalizeThirdPartyModuleUrl(documentUrl);
        const cachedTask = documentBlobCache.get(normalizedUrl);
        if (cachedTask) {
            return cachedTask;
        }

        const chain = new Set(processingChain);
        chain.add(normalizedUrl);
        const task = (async () => {
            const response = await doFetch(normalizedUrl, { cache: 'no-store' });
            if (!response.ok) {
                throw new Error(`Failed to fetch extension document: ${response.status} ${response.statusText}`);
            }

            const contentType = String(response.headers.get('content-type') || '').toLowerCase();
            const source = await response.text();
            if (!contentType.includes('text/html') && !looksLikeHtmlPayload(source)) {
                throw new Error(`Extension document is not HTML: ${normalizedUrl}`);
            }

            const rewritten = await rewriteThirdPartyDocumentSource(source, normalizedUrl, chain);
            const blobUrl = URL.createObjectURL(new Blob([rewritten], { type: 'text/html' }));
            documentBlobUrls.add(blobUrl);
            return blobUrl;
        })();

        documentBlobCache.set(normalizedUrl, task);

        try {
            return await task;
        } catch (error) {
            documentBlobCache.delete(normalizedUrl);
            throw error;
        }
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
            const source = normalizeModuleSource(await response.text());
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

    async function loadPreparedStylesheetSource(stylesheetUrl) {
        const doFetch = resolveFetchImpl(fetchImpl);
        const normalizedUrl = normalizeThirdPartyModuleUrl(stylesheetUrl);
        const cachedTask = stylesheetSourceCache.get(normalizedUrl);
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
            return absolutizeStylesheetUrls(preparedStylesheet, normalizedUrl);
        })();

        stylesheetSourceCache.set(normalizedUrl, task);

        try {
            return await task;
        } catch (error) {
            stylesheetSourceCache.delete(normalizedUrl);
            throw error;
        }
    }

    async function resolveStylesheetBlobUrl(stylesheetUrl) {
        const normalizedUrl = normalizeThirdPartyModuleUrl(stylesheetUrl);
        const cachedTask = styleBlobCache.get(normalizedUrl);
        if (cachedTask) {
            return cachedTask;
        }

        const task = (async () => {
            const stylesheetSource = await loadPreparedStylesheetSource(normalizedUrl);
            const blobUrl = URL.createObjectURL(new Blob([stylesheetSource], { type: 'text/css' }));
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
