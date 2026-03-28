// @ts-check

import { createTauriEventStreamBridge } from './tauri-event-stream-bridge.js';

const BACKEND_LOG_EVENT = 'tauritavern-backend-log';
const DEFAULT_BACKEND_LOG_TAIL_LIMIT = 800;

/**
 * @param {unknown} value
 * @param {string} label
 */
function requirePositiveInteger(value, label) {
    const number = typeof value === 'number' ? value : Number(value);
    if (!Number.isSafeInteger(number) || number <= 0) {
        throw new Error(`${label} must be a positive integer`);
    }
    return number;
}

/**
 * @param {{ safeInvoke: (command: any, args?: any) => Promise<any> }} deps
 */
export function createBackendLogBridge({ safeInvoke }) {
    const stream = createTauriEventStreamBridge({
        safeInvoke,
        enableCommand: 'devlog_set_backend_log_stream_enabled',
        eventName: BACKEND_LOG_EVENT,
    });

    return {
        /**
         * @param {{ limit?: number }} [options]
         */
        async tail(options = {}) {
            const limit = options?.limit === undefined
                ? DEFAULT_BACKEND_LOG_TAIL_LIMIT
                : requirePositiveInteger(options.limit, 'limit');

            return safeInvoke('devlog_get_backend_log_tail', { limit });
        },
        subscribe: stream.subscribe,
    };
}
