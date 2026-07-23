// @ts-check

/** @param {unknown} value */
function toError(value) {
    return value instanceof Error ? value : new Error(String(value));
}

export function createChatSurfaceFaultAuthority() {
    /** @type {Error | null} */
    let current = null;
    /** @type {Set<(error: Error) => void>} */
    const subscribers = new Set();

    /** @param {unknown} value */
    function set(value) {
        if (current === null) {
            current = toError(value);
            for (const subscriber of subscribers) {
                subscriber(current);
            }
        }
        return current;
    }

    /** @param {(error: Error) => void} subscriber */
    function subscribe(subscriber) {
        if (typeof subscriber !== 'function') {
            throw new TypeError('ChatSurface fault subscriber must be a function');
        }
        subscribers.add(subscriber);
        if (current) {
            subscriber(current);
        }
        return () => {
            subscribers.delete(subscriber);
        };
    }

    return Object.freeze({
        get current() { return current; },
        set,
        clear: () => { current = null; },
        subscribe,
    });
}
