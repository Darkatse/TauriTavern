import { getParsedUA, isMobile } from './RossAscends-mods.js';

const isFirefox = () => /firefox/i.test(navigator.userAgent);
let runtimeCompatibilityApplied = false;

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
