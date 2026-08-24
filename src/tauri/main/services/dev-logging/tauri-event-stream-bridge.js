// @ts-check

import { listen as tauriListen } from '../../../../tauri-bridge.js';

/**
 * @template T
 * @param {T} entry
 * @param {Set<(entry: T) => void>} subscribers
 */
function dispatch(entry, subscribers) {
    for (const handler of subscribers) {
        handler(entry);
    }
}

/**
 * Reference-counted owner of one host log stream. The stream is enabled while
 * at least one subscriber is attached and disabled once the last one leaves.
 *
 * @template T
 * @param {{
 *   safeInvoke: (command: any, args?: any) => Promise<any>;
 *   enableCommand: any;
 *   eventName: string;
 *   listen?: (eventName: string, handler: (event: { payload: T }) => void) => Promise<() => void>;
 * }} deps
 */
export function createTauriEventStreamBridge({ safeInvoke, enableCommand, eventName, listen = tauriListen }) {
    /** @type {Set<(entry: T) => void>} */
    const subscribers = new Set();
    /** @type {(() => void) | null} */
    let unlisten = null;
    /** @type {Promise<void> | null} */
    let starting = null;
    /** @type {Promise<void> | null} */
    let stopping = null;

    async function ensureStarted() {
        if (unlisten) {
            return;
        }

        if (starting) {
            await starting;
            return;
        }

        starting = (async () => {
            // A stop for the previous generation may still be in flight; the
            // re-enable must never overtake its disable. A failed disable
            // leaves the host stream running, which re-enabling heals, so it
            // must not block the new start.
            if (stopping) {
                try {
                    await stopping;
                } catch {
                    // See above: the stream is re-enabled right after.
                }
            }
            await safeInvoke(enableCommand, { enabled: true });
            try {
                unlisten = await listen(eventName, /** @param {{ payload: T }} event */ (event) => {
                    dispatch(/** @type {T} */ (event.payload), subscribers);
                });
            } catch (error) {
                await safeInvoke(enableCommand, { enabled: false });
                throw error;
            }
        })();

        try {
            await starting;
        } finally {
            starting = null;
        }
    }

    async function stopIfIdle() {
        if (subscribers.size > 0) {
            return;
        }

        if (starting) {
            try {
                await starting;
            } catch {
                // The start failure is reported to its subscriber; there is
                // nothing left running for this stop to tear down.
                return;
            }
            if (subscribers.size > 0) {
                return;
            }
        }

        if (!unlisten) {
            return;
        }

        const stopListening = unlisten;
        unlisten = null;
        stopping = (async () => {
            stopListening();
            await safeInvoke(enableCommand, { enabled: false });
        })();

        try {
            await stopping;
        } finally {
            stopping = null;
        }
    }

    /**
     * @param {(entry: T) => void} handler
     * @returns {Promise<() => Promise<void>>} idempotent disposer resolving once the stop it triggered has settled
     */
    async function subscribe(handler) {
        if (typeof handler !== 'function') {
            throw new Error('handler must be a function');
        }

        subscribers.add(handler);
        try {
            await ensureStarted();
        } catch (error) {
            // A rejected start must not leave a ghost subscriber that a later
            // successful start would dispatch to.
            subscribers.delete(handler);
            throw error;
        }

        let disposed = false;
        return () => {
            if (disposed) {
                return stopping ?? Promise.resolve();
            }
            disposed = true;
            subscribers.delete(handler);
            return stopIfIdle();
        };
    }

    return { subscribe };
}
