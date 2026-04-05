/**
 * A tiny "idle gate" primitive for generation lifecycles.
 *
 * Semantics:
 * - `wait()` resolves only when the gate is idle.
 * - `markBusy()` transitions idle -> busy and makes `wait()` pending.
 * - `markIdle()` transitions busy -> idle and resolves pending `wait()`.
 *
 * The gate is intentionally minimal and side-effect free, so it can be reused
 * across vendor and host layers and unit-tested in isolation.
 *
 * @returns {{
 *   wait: () => Promise<void>;
 *   markBusy: () => void;
 *   markIdle: () => void;
 * }}
 */
export function createGenerationIdleGate() {
    /** @type {Promise<void>} */
    let idlePromise = Promise.resolve();
    /** @type {(() => void) | null} */
    let resolveIdlePromise = null;

    const wait = () => idlePromise;

    const markBusy = () => {
        if (resolveIdlePromise) {
            return;
        }

        idlePromise = new Promise((resolve) => {
            resolveIdlePromise = resolve;
        });
    };

    const markIdle = () => {
        const resolve = resolveIdlePromise;
        if (!resolve) {
            return;
        }

        resolveIdlePromise = null;
        resolve();
    };

    return {
        wait,
        markBusy,
        markIdle,
    };
}

