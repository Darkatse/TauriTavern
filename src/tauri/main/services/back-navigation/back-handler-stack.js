// @ts-check

/** @type {Array<() => boolean>} */
const backHandlers = [];

/**
 * Registers a temporary back-navigation handler.
 *
 * Handlers are evaluated in LIFO order and must be synchronous because the
 * native Android back bridge expects an immediate boolean result.
 *
 * @param {() => boolean} handler
 * @returns {() => void}
 */
export function pushBackNavigationHandler(handler) {
    if (typeof handler !== 'function') {
        throw new Error('Back navigation handler must be a function');
    }

    backHandlers.push(handler);

    let active = true;
    return () => {
        if (!active) {
            return;
        }
        active = false;

        const index = backHandlers.lastIndexOf(handler);
        if (index >= 0) {
            backHandlers.splice(index, 1);
        }
    };
}

export function consumeBackNavigationHandlers() {
    for (let index = backHandlers.length - 1; index >= 0; index -= 1) {
        const handler = backHandlers[index];
        if (handler?.()) {
            return true;
        }
    }

    return false;
}
