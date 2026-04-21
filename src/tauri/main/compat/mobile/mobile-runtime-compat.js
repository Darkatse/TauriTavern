const COMPAT_KEY = '__TAURITAVERN_MOBILE_RUNTIME_COMPAT__';

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

function now() {
    if (typeof performance !== 'undefined' && performance && typeof performance.now === 'function') {
        return performance.now();
    }

    return Date.now();
}

function requestIdleCallbackPolyfill(callback, options) {
    if (typeof callback !== 'function') {
        throw new TypeError('requestIdleCallback: callback must be a function');
    }

    const start = now();
    const rawTimeout = options && typeof options === 'object' ? Number(options.timeout) : NaN;
    const timeoutMs = Number.isFinite(rawTimeout) && rawTimeout > 0 ? rawTimeout : null;
    const budgetMs = 50;

    return setTimeout(function idleCallbackTimer() {
        const deadline = {
            didTimeout: timeoutMs !== null && (now() - start) >= timeoutMs,
            timeRemaining: function timeRemaining() {
                return Math.max(0, budgetMs - (now() - start));
            },
        };

        callback(deadline);
    }, 1);
}

function cancelIdleCallbackPolyfill(handle) {
    clearTimeout(handle);
}

function toIntegerOrInfinity(value) {
    const number = Number(value);
    if (Number.isNaN(number) || number === 0) {
        return 0;
    }

    if (!Number.isFinite(number)) {
        return number;
    }

    return Math.trunc(number);
}

function normalizeIndex(length, index) {
    const integer = toIntegerOrInfinity(index);
    return integer >= 0 ? integer : length + integer;
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

export function installMobileRuntimeCompat() {
    if (window[COMPAT_KEY]) {
        return;
    }
    window[COMPAT_KEY] = true;

    const needsRequestIdleCallback = typeof window.requestIdleCallback !== 'function';
    const needsCancelIdleCallback = typeof window.cancelIdleCallback !== 'function';
    defineMissingMethod(window, 'requestIdleCallback', requestIdleCallbackPolyfill);
    defineMissingMethod(window, 'cancelIdleCallback', cancelIdleCallbackPolyfill);
    if (needsRequestIdleCallback || needsCancelIdleCallback) {
        console.warn('[TauriTavern] Mobile runtime compat installed a requestIdleCallback polyfill.');
    }

    defineMissingMethod(Array.prototype, 'at', atPolyfill);
    defineMissingMethod(String.prototype, 'at', atPolyfill);
    defineMissingMethod(Array.prototype, 'findLast', findLastPolyfill);
    defineMissingMethod(Array.prototype, 'findLastIndex', findLastIndexPolyfill);
    defineMissingMethod(Array.prototype, 'toSorted', toSortedPolyfill);
    defineMissingMethod(Array.prototype, 'toReversed', toReversedPolyfill);
    defineMissingMethod(Object, 'hasOwn', hasOwnPolyfill);
}
