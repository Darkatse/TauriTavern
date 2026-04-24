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

function now(targetWindow) {
    try {
        if (targetWindow?.performance && typeof targetWindow.performance.now === 'function') {
            return targetWindow.performance.now();
        }
    } catch {
        // Ignore cross-origin access failures.
    }

    return Date.now();
}

function createRequestIdleCallbackPolyfill(targetWindow) {
    return function requestIdleCallbackPolyfill(callback, options) {
        if (typeof callback !== 'function') {
            throw new TypeError('requestIdleCallback: callback must be a function');
        }

        const start = now(targetWindow);
        const rawTimeout = options && typeof options === 'object' ? Number(options.timeout) : NaN;
        const timeoutMs = Number.isFinite(rawTimeout) && rawTimeout > 0 ? rawTimeout : null;
        const budgetMs = 50;

        const setTimeoutFn = (() => {
            if (typeof targetWindow?.setTimeout === 'function') {
                return targetWindow.setTimeout.bind(targetWindow);
            }
            return setTimeout;
        })();

        return setTimeoutFn(() => {
            const deadline = {
                didTimeout: timeoutMs !== null && (now(targetWindow) - start) >= timeoutMs,
                timeRemaining: function timeRemaining() {
                    return Math.max(0, budgetMs - (now(targetWindow) - start));
                },
            };

            callback.call(targetWindow, deadline);
        }, 1);
    };
}

function createCancelIdleCallbackPolyfill(targetWindow) {
    return function cancelIdleCallbackPolyfill(handle) {
        if (typeof targetWindow?.clearTimeout === 'function') {
            targetWindow.clearTimeout(handle);
            return;
        }

        clearTimeout(handle);
    };
}

function defineMissingGlobalMethod(targetWindow, key, implementation) {
    if (!targetWindow || typeof targetWindow[key] === 'function') {
        return false;
    }

    try {
        targetWindow[key] = implementation;
        return true;
    } catch {
        return false;
    }
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

export function installMobileRuntimeCompat(targetWindow = window) {
    if (!targetWindow) {
        return;
    }

    try {
        if (targetWindow[COMPAT_KEY]) {
            return;
        }
        targetWindow[COMPAT_KEY] = true;
    } catch {
        return;
    }

    const needsRequestIdleCallback = typeof targetWindow.requestIdleCallback !== 'function';
    const needsCancelIdleCallback = typeof targetWindow.cancelIdleCallback !== 'function';
    const installedIdle = defineMissingGlobalMethod(
        targetWindow,
        'requestIdleCallback',
        createRequestIdleCallbackPolyfill(targetWindow),
    );
    const installedCancel = defineMissingGlobalMethod(
        targetWindow,
        'cancelIdleCallback',
        createCancelIdleCallbackPolyfill(targetWindow),
    );
    if ((needsRequestIdleCallback && installedIdle) || (needsCancelIdleCallback && installedCancel)) {
        console.warn('[TauriTavern] Mobile runtime compat installed a requestIdleCallback polyfill.');
    }

    defineMissingMethod(targetWindow.Array?.prototype, 'at', atPolyfill);
    defineMissingMethod(targetWindow.String?.prototype, 'at', atPolyfill);
    defineMissingMethod(targetWindow.Array?.prototype, 'findLast', findLastPolyfill);
    defineMissingMethod(targetWindow.Array?.prototype, 'findLastIndex', findLastIndexPolyfill);
    defineMissingMethod(targetWindow.Array?.prototype, 'toSorted', toSortedPolyfill);
    defineMissingMethod(targetWindow.Array?.prototype, 'toReversed', toReversedPolyfill);
    defineMissingMethod(targetWindow.Object, 'hasOwn', hasOwnPolyfill);
}
