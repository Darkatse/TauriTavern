// @ts-check

import { createChannel } from '../../../tauri-bridge.js';

/**
 * Each subscription opens its own host live projection handle and streams it over its own Channel. The handle is
 * closed once, when the subscriber unsubscribes or the stream ends; closing it ends the stream. The Channel is created
 * only for the stream call: the host ends every Channel it receives, which unregisters its callback.
 * @param {{
 *   safeInvoke: (command: string, args?: any) => Promise<any>;
 *   channelFactory?: (onmessage: (update: any) => void) => any;
 * }} deps
 */
export function createAgentRunLiveSubscribe({ safeInvoke, channelFactory = createChannel }) {
    return function subscribeLiveProjection(runId, handler, options = {}) {
        const normalizedRunId = requireRunId(runId);
        if (typeof handler !== 'function') {
            throw new Error('handler is required');
        }
        const onError = options?.onError;

        let active = true;
        /** @type {number | null} */
        let rid = null;
        /** @type {Promise<void> | null} */
        let closing = null;
        const deliver = (update) => {
            if (!active) {
                return;
            }
            try {
                handler(update);
            } catch (error) {
                reportError(error, onError);
            }
        };
        const close = () => {
            closing ??= Promise.resolve(safeInvoke('plugin:resources|close', { rid }))
                .then(() => undefined, error => reportError(error, onError));
            return closing;
        };

        void (async () => {
            try {
                rid = await safeInvoke('open_agent_run_live_projection', { dto: { runId: normalizedRunId } });
                if (active) {
                    await safeInvoke('stream_agent_run_live_projection', { rid, channel: channelFactory(deliver) });
                }
            } catch (error) {
                if (active) {
                    reportError(error, onError);
                }
            } finally {
                active = false;
                if (rid != null) {
                    await close();
                }
            }
        })();

        return function unsubscribe() {
            active = false;
            if (rid != null) {
                void close();
            }
        };
    };
}

function reportError(error, onError) {
    if (typeof onError === 'function') {
        try {
            onError(error);
            return;
        } catch (callbackError) {
            error = callbackError;
        }
    }
    queueMicrotask(() => {
        throw error;
    });
}

function requireRunId(value) {
    const runId = String(value || '').trim();
    if (!runId) {
        throw new Error('runId is required');
    }
    return runId;
}
