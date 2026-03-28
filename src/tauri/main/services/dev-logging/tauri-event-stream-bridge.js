// @ts-check

import { listen } from '../../../../tauri-bridge.js';

/**
 * @template T
 * @param {{
 *   safeInvoke: (command: any, args?: any) => Promise<any>;
 *   enableCommand: any;
 *   eventName: string;
 * }} deps
 */
export function createTauriEventStreamBridge({ safeInvoke, enableCommand, eventName }) {
    /** @type {Set<(entry: T) => void>} */
    const subscribers = new Set();
    /** @type {(() => void) | null} */
    let unlisten = null;
    /** @type {Promise<void> | null} */
    let starting = null;

    /**
     * @param {T} entry
     */
    function dispatch(entry) {
        for (const handler of subscribers) {
            handler(entry);
        }
    }

    async function ensureStarted() {
        if (unlisten) {
            return;
        }

        if (starting) {
            await starting;
            return;
        }

        starting = (async () => {
            await safeInvoke(enableCommand, { enabled: true });
            try {
                unlisten = await listen(eventName, /** @param {{ payload: T }} event */ (event) => {
                    dispatch(/** @type {T} */ (event.payload));
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
            await starting;
            if (subscribers.size > 0) {
                return;
            }
        }

        if (!unlisten) {
            return;
        }

        const stopListening = unlisten;
        unlisten = null;
        stopListening();
        await safeInvoke(enableCommand, { enabled: false });
    }

    /**
     * @param {(entry: T) => void} handler
     */
    async function subscribe(handler) {
        if (typeof handler !== 'function') {
            throw new Error('handler must be a function');
        }

        subscribers.add(handler);
        await ensureStarted();

        return () => {
            subscribers.delete(handler);
            void stopIfIdle();
        };
    }

    return { subscribe };
}
