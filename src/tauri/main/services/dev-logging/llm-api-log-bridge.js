// @ts-check

import { createTauriEventStreamBridge } from './tauri-event-stream-bridge.js';

const LLM_API_LOG_EVENT = 'tauritavern-llm-api-log';

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
export function createLlmApiLogBridge({ safeInvoke }) {
    const stream = createTauriEventStreamBridge({
        safeInvoke,
        enableCommand: 'devlog_set_llm_api_log_stream_enabled',
        eventName: LLM_API_LOG_EVENT,
    });

    return {
        /**
         * @param {{ limit?: number }} [options]
         */
        async index(options = {}) {
            if (options?.limit === undefined) {
                return safeInvoke('devlog_get_llm_api_log_index');
            }

            return safeInvoke('devlog_get_llm_api_log_index', {
                limit: requirePositiveInteger(options.limit, 'limit'),
            });
        },
        /**
         * @param {number} id
         */
        async getPreview(id) {
            return safeInvoke('devlog_get_llm_api_log_preview', {
                id: requirePositiveInteger(id, 'id'),
            });
        },
        /**
         * @param {number} id
         */
        async getRaw(id) {
            return safeInvoke('devlog_get_llm_api_log_raw', {
                id: requirePositiveInteger(id, 'id'),
            });
        },
        subscribeIndex: stream.subscribe,
    };
}
