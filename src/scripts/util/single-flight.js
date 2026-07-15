/**
 * Shares an in-flight promise for callers using the same exact key.
 * Settled results are never retained.
 *
 * @template T
 * @returns {(key: string, task: () => Promise<T>) => Promise<T>}
 */
export function createSingleFlight() {
    /** @type {Map<string, Promise<T>>} */
    const inFlight = new Map();

    return function runSingleFlight(key, task) {
        const existing = inFlight.get(key);
        if (existing) return existing;

        const promise = Promise.resolve().then(task);
        inFlight.set(key, promise);
        void promise.finally(() => {
            if (inFlight.get(key) === promise) {
                inFlight.delete(key);
            }
        }).catch(() => {});
        return promise;
    };
}
