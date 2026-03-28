// @ts-check

import { createBackendLogBridge } from '../services/dev-logging/backend-log-bridge.js';
import {
    getFrontendLogEntries,
    isFrontendConsoleCaptureEnabled,
    setFrontendConsoleCaptureEnabled,
    subscribeFrontendLogs,
} from '../services/dev-logging/frontend-log-capture.js';
import { createLlmApiLogBridge } from '../services/dev-logging/llm-api-log-bridge.js';

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
 * @template T
 * @param {T[]} entries
 * @param {{ limit?: number }} options
 */
function sliceTail(entries, options = {}) {
    if (options.limit === undefined) {
        return entries.slice();
    }

    const limit = requirePositiveInteger(options.limit, 'limit');
    return entries.slice(-limit);
}

/**
 * @param {{ safeInvoke: (command: any, args?: any) => Promise<any> }} deps
 */
function createDevSettingsStore({ safeInvoke }) {
    return {
        async read() {
            const settings = await safeInvoke('get_tauritavern_settings');
            return settings.dev;
        },
        async update(patch) {
            const settings = await safeInvoke('update_tauritavern_settings', {
                dto: {
                    dev: patch,
                },
            });
            return settings.dev;
        },
    };
}

/**
 * @param {{ settings: ReturnType<typeof createDevSettingsStore> }} deps
 */
function createFrontendLogsApi({ settings }) {
    return {
        async list(options = {}) {
            return sliceTail(getFrontendLogEntries(), options);
        },
        async subscribe(handler) {
            if (typeof handler !== 'function') {
                throw new Error('handler must be a function');
            }

            return subscribeFrontendLogs(handler);
        },
        async getConsoleCaptureEnabled() {
            const { frontend_console_capture: enabled } = await settings.read();
            if (enabled !== isFrontendConsoleCaptureEnabled()) {
                setFrontendConsoleCaptureEnabled(enabled);
            }
            return enabled;
        },
        async setConsoleCaptureEnabled(enabled) {
            const nextEnabled = Boolean(enabled);
            await settings.update({
                frontend_console_capture: nextEnabled,
            });
            setFrontendConsoleCaptureEnabled(nextEnabled);
        },
    };
}

/**
 * @param {{ safeInvoke: (command: any, args?: any) => Promise<any> }} deps
 */
function createDevApi({ safeInvoke }) {
    const settings = createDevSettingsStore({ safeInvoke });
    const frontendLogs = createFrontendLogsApi({ settings });
    const backendLogs = createBackendLogBridge({ safeInvoke });
    const llmApiLogsBridge = createLlmApiLogBridge({ safeInvoke });

    return {
        frontendLogs,
        backendLogs,
        llmApiLogs: {
            ...llmApiLogsBridge,
            async getKeep() {
                const { llm_api_keep: keep } = await settings.read();
                return keep;
            },
            async setKeep(value) {
                await settings.update({
                    llm_api_keep: requirePositiveInteger(value, 'value'),
                });
            },
        },
    };
}

/**
 * @param {any} context
 */
export function installDevApi(context) {
    const hostWindow = /** @type {any} */ (window);
    const hostAbi = hostWindow.__TAURITAVERN__;
    if (!hostAbi || typeof hostAbi !== 'object') {
        throw new Error('Host ABI __TAURITAVERN__ is missing');
    }

    const safeInvoke = context?.safeInvoke;
    if (typeof safeInvoke !== 'function') {
        throw new Error('Tauri main context safeInvoke is missing');
    }

    if (!hostAbi.api || typeof hostAbi.api !== 'object') {
        hostAbi.api = {};
    }

    hostAbi.api.dev = createDevApi({ safeInvoke });
}
