// @ts-check

import { createChannel } from '../../../tauri-bridge.js';

/**
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

        let active = true;
        const channel = channelFactory((update) => {
            if (!active) {
                return;
            }
            try {
                handler(update);
            } catch (error) {
                reportError(error, options?.onError);
            }
        });
        const completion = Promise.resolve(safeInvoke('subscribe_agent_run_live_projection', {
            dto: { runId: normalizedRunId },
            channel,
        }));
        completion.then(
            () => {
                active = false;
            },
            (error) => {
                if (!active) {
                    return;
                }
                active = false;
                reportError(error, options?.onError);
            },
        );

        return function unsubscribe() {
            active = false;
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
