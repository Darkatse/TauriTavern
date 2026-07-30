// @ts-check

/** @param {unknown} value */
function isThenable(value) {
    return Boolean(
        value
        && (typeof value === 'object' || typeof value === 'function')
        && typeof /** @type {{ then?: unknown }} */ (value).then === 'function'
    );
}

/**
 * Owns one exact ChatSurface resource lifetime. Closing is synchronous,
 * idempotent and LIFO; all cleanups run even when one fails.
 *
 */
export function createResourceLease() {
    const abortController = new AbortController();
    /** @type {Array<() => unknown>} */
    const cleanups = [];
    let closed = false;
    /** @type {unknown} */
    let closeFailure;

    /** @param {unknown} disposable */
    function add(disposable) {
        if (closed) {
            throw new Error('Cannot add a disposable to a closed ChatSurface lease');
        }
        if (typeof disposable === 'function') {
            cleanups.push(/** @type {() => unknown} */ (disposable));
            return;
        }
        if (disposable && typeof disposable === 'object' && typeof /** @type {{ dispose?: unknown }} */ (disposable).dispose === 'function') {
            cleanups.push(() => /** @type {{ dispose: () => unknown }} */ (disposable).dispose());
            return;
        }
        throw new TypeError('ChatSurface lease accepts a cleanup function or disposable object');
    }

    /** @param {string} [reason] */
    function close(reason = 'closed') {
        if (closed) {
            if (closeFailure !== undefined) {
                throw closeFailure;
            }
            return;
        }
        closed = true;
        abortController.abort(String(reason));

        /** @type {unknown} */
        let firstFailure;
        for (let index = cleanups.length - 1; index >= 0; index -= 1) {
            try {
                const result = cleanups[index]?.();
                if (isThenable(result)) {
                    throw new TypeError('ChatSurface cleanup must be synchronous');
                }
            } catch (error) {
                firstFailure ??= error;
            }
        }
        cleanups.length = 0;
        if (firstFailure !== undefined) {
            closeFailure = firstFailure;
            throw firstFailure;
        }
    }

    return Object.freeze({
        signal: abortController.signal,
        add,
        close,
    });
}
