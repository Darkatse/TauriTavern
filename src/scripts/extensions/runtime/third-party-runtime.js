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

function normalizeStylesheetUrl(stylesheetUrl) {
    const parsed = new URL(String(stylesheetUrl), window.location.origin);
    parsed.hash = '';
    return parsed.href;
}

function toLayerCompatStylesheetUrl(stylesheetUrl) {
    const parsed = new URL(String(stylesheetUrl), window.location.origin);
    parsed.hash = '';
    parsed.searchParams.set('ttCompat', 'layer');
    return parsed.href;
}

export function createThirdPartyStylesheetResolver() {
    function cleanup() {
        // No-op: kept for API compatibility with earlier blob-based implementation.
    }

    async function resolveStylesheetUrl(stylesheetUrl) {
        const normalizedUrl = normalizeStylesheetUrl(stylesheetUrl);
        if (supportsCssCascadeLayers()) {
            return normalizedUrl;
        }

        return toLayerCompatStylesheetUrl(normalizedUrl);
    }

    return {
        resolveStylesheetUrl,
        cleanup,
    };
}

