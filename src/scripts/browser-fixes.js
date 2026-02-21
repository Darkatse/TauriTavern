import { getParsedUA, isMobile } from './RossAscends-mods.js';

const isFirefox = () => /firefox/i.test(navigator.userAgent);
let runtimeCompatibilityApplied = false;
let mobileSafeAreaStylePatchApplied = false;

const CSS_RULE_BLOCK_PATTERN = /([^{}]+)\{([^{}]*)\}/g;
const CSS_FIXED_POSITION_PATTERN = /\bposition\s*:\s*fixed\b/i;
const CSS_TOP_DECLARATION_PATTERN = /\btop\s*:\s*([^;{}]+)(;?)/gi;
const CSS_SAFE_AREA_TOP_REFERENCE_PATTERN = /(?:--tt-safe-area-top|safe-area-inset-top)/i;
const CSS_NON_NUMERIC_TOP_VALUE_PATTERN = /^(auto|inherit|initial|unset|revert|revert-layer)$/i;
const CSS_ZERO_TOP_VALUE_PATTERN = /^0(?:\.0+)?(?:[a-z%]+)?$/i;
const observedDynamicStyles = new WeakSet();
const MOBILE_SAFE_AREA_SKIP_ELEMENT_IDS = new Set(['top-settings-holder', 'top-bar']);
const MOBILE_FIXED_TOP_CANDIDATE_SELECTOR = [
    '[style*="top"]',
    '[class*="popup"]',
    '[class*="modal"]',
    '[class*="window"]',
    '[id*="popup"]',
    '[id*="modal"]',
    '[id*="window"]',
].join(', ');

function defineMissingMethod(target, key, implementation) {
    if (!target || typeof target[key] === 'function') {
        return;
    }

    Object.defineProperty(target, key, {
        value: implementation,
        configurable: true,
        writable: true,
    });
}

function toInteger(value) {
    const numeric = Number(value);
    if (!Number.isFinite(numeric) || numeric === 0) {
        return 0;
    }

    return Math.trunc(numeric);
}

function normalizeIndex(length, index) {
    const normalized = toInteger(index);
    return normalized >= 0 ? normalized : length + normalized;
}

function atPolyfill(index) {
    if (this == null) {
        throw new TypeError('Cannot convert undefined or null to object');
    }

    const target = Object(this);
    const length = target.length >>> 0;
    const resolvedIndex = normalizeIndex(length, index);
    if (resolvedIndex < 0 || resolvedIndex >= length) {
        return undefined;
    }

    return target[resolvedIndex];
}

function findLastIndexPolyfill(predicate, thisArg) {
    if (this == null) {
        throw new TypeError('Cannot convert undefined or null to object');
    }

    if (typeof predicate !== 'function') {
        throw new TypeError('Predicate must be a function');
    }

    const target = Object(this);
    const length = target.length >>> 0;
    for (let index = length - 1; index >= 0; index -= 1) {
        if (!(index in target)) {
            continue;
        }

        if (predicate.call(thisArg, target[index], index, target)) {
            return index;
        }
    }

    return -1;
}

function findLastPolyfill(predicate, thisArg) {
    const index = findLastIndexPolyfill.call(this, predicate, thisArg);
    return index === -1 ? undefined : this[index];
}

function toSortedPolyfill(compareFn) {
    if (compareFn !== undefined && typeof compareFn !== 'function') {
        throw new TypeError('Comparator must be a function');
    }

    return Array.from(this).sort(compareFn);
}

function toReversedPolyfill() {
    return Array.from(this).reverse();
}

function hasOwnPolyfill(target, property) {
    if (target == null) {
        throw new TypeError('Cannot convert undefined or null to object');
    }

    return Object.prototype.hasOwnProperty.call(Object(target), property);
}

function needsRuntimeCompatibility() {
    return typeof Array.prototype.at !== 'function'
        || typeof String.prototype.at !== 'function'
        || typeof Array.prototype.findLast !== 'function'
        || typeof Array.prototype.findLastIndex !== 'function'
        || typeof Array.prototype.toSorted !== 'function'
        || typeof Array.prototype.toReversed !== 'function'
        || typeof Object.hasOwn !== 'function';
}

function applyMobileRuntimeCompatibility() {
    if (runtimeCompatibilityApplied || !isMobile() || !needsRuntimeCompatibility()) {
        return;
    }

    runtimeCompatibilityApplied = true;
    defineMissingMethod(Array.prototype, 'at', atPolyfill);
    defineMissingMethod(String.prototype, 'at', atPolyfill);
    defineMissingMethod(Array.prototype, 'findLast', findLastPolyfill);
    defineMissingMethod(Array.prototype, 'findLastIndex', findLastIndexPolyfill);
    defineMissingMethod(Array.prototype, 'toSorted', toSortedPolyfill);
    defineMissingMethod(Array.prototype, 'toReversed', toReversedPolyfill);
    defineMissingMethod(Object, 'hasOwn', hasOwnPolyfill);
}

function normalizeSafeAreaTopValue(rawValue) {
    const value = String(rawValue || '').trim();
    if (!value || CSS_SAFE_AREA_TOP_REFERENCE_PATTERN.test(value)) {
        return null;
    }

    const importantMatch = value.match(/\s*!important\s*$/i);
    const importantSuffix = importantMatch ? ' !important' : '';
    const baseValue = value.replace(/\s*!important\s*$/i, '').trim();
    if (!baseValue || CSS_NON_NUMERIC_TOP_VALUE_PATTERN.test(baseValue)) {
        return null;
    }

    const normalizedBaseValue = CSS_ZERO_TOP_VALUE_PATTERN.test(baseValue) ? '0px' : baseValue;
    return `max(var(--tt-safe-area-top), ${normalizedBaseValue})${importantSuffix}`;
}

function rewriteFixedTopDeclarations(declarations) {
    const source = String(declarations || '');
    if (!CSS_FIXED_POSITION_PATTERN.test(source) || !/\btop\s*:/i.test(source)) {
        return source;
    }

    let changed = false;
    const rewritten = source.replace(CSS_TOP_DECLARATION_PATTERN, (fullMatch, topValue, semicolon) => {
        const safeAreaTopValue = normalizeSafeAreaTopValue(topValue);
        if (!safeAreaTopValue) {
            return fullMatch;
        }

        changed = true;
        return `top: ${safeAreaTopValue}${semicolon || ';'}`;
    });

    return changed ? rewritten : source;
}

function rewriteDynamicStyleSafeArea(cssText) {
    const source = String(cssText || '');
    if (!source || !CSS_FIXED_POSITION_PATTERN.test(source) || !/\btop\s*:/i.test(source)) {
        return source;
    }

    let changed = false;
    const rewritten = source.replace(CSS_RULE_BLOCK_PATTERN, (fullMatch, selector, declarations) => {
        const rewrittenDeclarations = rewriteFixedTopDeclarations(declarations);
        if (rewrittenDeclarations === declarations) {
            return fullMatch;
        }

        changed = true;
        return `${selector}{${rewrittenDeclarations}}`;
    });

    return changed ? rewritten : source;
}

function patchDynamicStyleElement(styleElement) {
    if (!(styleElement instanceof HTMLStyleElement)) {
        return;
    }

    const source = String(styleElement.textContent || '');
    const rewritten = rewriteDynamicStyleSafeArea(source);
    if (rewritten !== source) {
        styleElement.textContent = rewritten;
    }
}

function observeDynamicStyleElement(styleElement) {
    if (!(styleElement instanceof HTMLStyleElement) || observedDynamicStyles.has(styleElement)) {
        return;
    }

    observedDynamicStyles.add(styleElement);
    patchDynamicStyleElement(styleElement);

    const observer = new MutationObserver(() => {
        patchDynamicStyleElement(styleElement);
    });
    observer.observe(styleElement, {
        childList: true,
        characterData: true,
        subtree: true,
    });
}

function collectDynamicStyleNodes(node) {
    if (!(node instanceof Element)) {
        return [];
    }

    if (node instanceof HTMLStyleElement) {
        return [node];
    }

    return Array.from(node.querySelectorAll('style'));
}

function parsePixelValue(rawValue) {
    const value = String(rawValue || '').trim();
    const match = value.match(/^(-?\d+(?:\.\d+)?)px$/i);
    if (!match) {
        return null;
    }

    const parsed = Number(match[1]);
    return Number.isFinite(parsed) ? parsed : null;
}

function getSafeAreaTopPx() {
    const rootStyle = getComputedStyle(document.documentElement);
    const safeAreaTop = parsePixelValue(rootStyle.getPropertyValue('--tt-safe-area-top'));
    return safeAreaTop === null ? 0 : safeAreaTop;
}

function shouldSkipSafeAreaPatch(element) {
    return MOBILE_SAFE_AREA_SKIP_ELEMENT_IDS.has(element.id);
}

function resolveElementTopForPatch(element, computedStyle) {
    const inlineTop = String(element.style.getPropertyValue('top') || '').trim();
    if (inlineTop) {
        return {
            value: inlineTop,
        };
    }

    const computedTop = String(computedStyle.top || '').trim();
    const computedTopPx = parsePixelValue(computedTop);
    const safeAreaTopPx = getSafeAreaTopPx();
    if (computedTopPx === null || computedTopPx < 0 || computedTopPx >= safeAreaTopPx) {
        return null;
    }

    return {
        value: computedTop,
    };
}

function patchFixedElementTopForSafeArea(element) {
    if (!(element instanceof HTMLElement) || shouldSkipSafeAreaPatch(element)) {
        return;
    }

    const computedStyle = getComputedStyle(element);
    if (computedStyle.position !== 'fixed') {
        return;
    }

    const top = resolveElementTopForPatch(element, computedStyle);
    if (!top) {
        return;
    }

    const safeAreaTopValue = normalizeSafeAreaTopValue(top.value);
    if (!safeAreaTopValue) {
        return;
    }
    element.style.setProperty('top', safeAreaTopValue, 'important');
}

function patchFixedElementTopInTree(node) {
    if (!(node instanceof Element)) {
        return;
    }

    if (node instanceof HTMLElement) {
        patchFixedElementTopForSafeArea(node);
    }

    for (const element of node.querySelectorAll(MOBILE_FIXED_TOP_CANDIDATE_SELECTOR)) {
        if (element instanceof HTMLElement) {
            patchFixedElementTopForSafeArea(element);
        }
    }
}

function applyMobileDynamicStyleSafeAreaPatch() {
    if (mobileSafeAreaStylePatchApplied || !isMobile() || typeof MutationObserver !== 'function') {
        return;
    }

    mobileSafeAreaStylePatchApplied = true;
    patchFixedElementTopInTree(document.body);

    const observer = new MutationObserver(mutations => {
        for (const mutation of mutations) {
            if (mutation.type === 'attributes' && mutation.target instanceof HTMLElement) {
                patchFixedElementTopForSafeArea(mutation.target);
                continue;
            }

            for (const node of mutation.addedNodes) {
                for (const styleElement of collectDynamicStyleNodes(node)) {
                    observeDynamicStyleElement(styleElement);
                }

                patchFixedElementTopInTree(node);
            }
        }
    });

    observer.observe(document.documentElement, {
        childList: true,
        subtree: true,
        attributes: true,
        attributeFilter: ['style', 'class'],
    });
}

function sanitizeInlineQuotationOnCopy() {
    // STRG+C, STRG+V on firefox leads to duplicate double quotes when inline quotation elements are copied.
    // To work around this, take the selection and transform <q> to <span> before calling toString().
    document.addEventListener('copy', function (event) {
        if (document.activeElement instanceof HTMLInputElement || document.activeElement instanceof HTMLTextAreaElement) {
            return;
        }

        const selection = window.getSelection();
        if (!selection.anchorNode?.parentElement.closest('.mes_text')) {
            return;
        }

        const range = selection.getRangeAt(0).cloneContents();
        const tempDOM = document.createDocumentFragment();

        /**
         * Process a node, transforming <q> elements to <span> elements and preserving children.
         * @param {Node} node Input node
         * @returns {Node} Processed node
         */
        function processNode(node) {
            if (node.nodeType === Node.ELEMENT_NODE && node.nodeName.toLowerCase() === 'q') {
                // Transform <q> to <span>, preserve children
                const span = document.createElement('span');

                [...node.childNodes].forEach(child => {
                    const processedChild = processNode(child);
                    span.appendChild(processedChild);
                });

                return span;
            } else {
                // Nested structures containing <q> elements are unlikely
                return node.cloneNode(true);
            }
        }

        [...range.childNodes].forEach(child => {
            const processedChild = processNode(child);
            tempDOM.appendChild(processedChild);
        });

        const newRange = document.createRange();
        newRange.selectNodeContents(tempDOM);

        event.preventDefault();
        event.clipboardData.setData('text/plain', newRange.toString());
    });
}

function addSafariPatch() {
    const userAgent = getParsedUA();
    console.debug('User Agent', userAgent);
    const isMobileSafari = /iPad|iPhone|iPod/.test(navigator.platform) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
    const isDesktopSafari = userAgent?.browser?.name === 'Safari' && userAgent?.platform?.type === 'desktop';
    const isIOS = userAgent?.os?.name === 'iOS';

    if (isIOS || isMobileSafari || isDesktopSafari) {
        document.body.classList.add('safari');
    }
}

function applyBrowserFixes() {
    applyMobileRuntimeCompatibility();
    applyMobileDynamicStyleSafeAreaPatch();

    if (isFirefox()) {
        sanitizeInlineQuotationOnCopy();
    }

    if (isMobile()) {
        const fixFunkyPositioning = () => {
            console.debug('[Mobile] Device viewport change detected.');
            document.documentElement.style.position = 'fixed';
            requestAnimationFrame(() => document.documentElement.style.position = '');
        };
        window.addEventListener('resize', fixFunkyPositioning);
        window.addEventListener('orientationchange', fixFunkyPositioning);
    }

    addSafariPatch();
}

export { isFirefox, applyBrowserFixes };
