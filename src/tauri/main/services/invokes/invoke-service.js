// @ts-check

import { createInvokeBroker } from '../../brokers/invoke-broker.js';

/**
 * @typedef {import('../../context/types.js').TauriInvokeFn} TauriInvokeFn
 * @typedef {import('../../context/types.js').TauriInvokeCommand} TauriInvokeCommand
 */

/**
 * @param {{
 *   invoke: TauriInvokeFn;
 *   policies: Record<string, any>;
 * }} deps
 */
export function createInvokeService({ invoke, policies }) {
    /** @param {number} ms */
    function sleep(ms) {
        return new Promise((resolve) => setTimeout(resolve, ms));
    }

    /** @param {Record<string, any> | null | undefined} args */
    function withTauriArgumentAliases(args) {
        if (!args || typeof args !== 'object' || Array.isArray(args)) {
            return args;
        }

        /** @type {Record<string, any> | null} */
        let aliased = null;

        for (const [key, value] of Object.entries(args)) {
            if (!key.includes('_')) {
                continue;
            }

            const target = aliased ?? (aliased = { ...args });

            const camelCaseKey = key.replace(/_+([a-zA-Z0-9])/g, (_, char) => char.toUpperCase());
            if (!Object.prototype.hasOwnProperty.call(target, camelCaseKey)) {
                target[camelCaseKey] = value;
            }
        }

        return aliased || args;
    }

    /** @param {unknown} message */
    function shouldRetryInvoke(message) {
        const normalized = String(message || '').toLowerCase();
        return (
            (normalized.includes('state') && normalized.includes('not managed')) ||
            normalized.includes('invoke is unavailable')
        );
    }

    /**
     * @param {unknown} value
     * @param {number} depth
     * @returns {string}
     */
    function extractErrorText(value, depth = 0) {
        if (depth > 4 || value === null || value === undefined) {
            return '';
        }

        if (typeof value === 'string') {
            return value.trim();
        }

        if (typeof value === 'number' || typeof value === 'boolean') {
            return String(value);
        }

        if (value instanceof Error) {
            const nested = extractErrorText(value.message, depth + 1);
            return nested || String(value).trim();
        }

        if (Array.isArray(value)) {
            for (const item of value) {
                const nested = extractErrorText(item, depth + 1);
                if (nested) {
                    return nested;
                }
            }
            return '';
        }

        if (typeof value === 'object') {
            const commandError = extractCommandErrorText(value, depth + 1);
            if (commandError) {
                return commandError;
            }

            const keys = ['message', 'error', 'details', 'reason', 'cause', 'data'];
            for (const key of keys) {
                if (Object.prototype.hasOwnProperty.call(value, key)) {
                    // @ts-ignore - dynamic object indexing by contract.
                    const nested = extractErrorText(value[key], depth + 1);
                    if (nested) {
                        return nested;
                    }
                }
            }
        }

        return '';
    }

    /**
     * @param {Record<string, any>} value
     * @param {number} depth
     */
    function extractCommandErrorText(value, depth) {
        if (depth > 4 || !value || Array.isArray(value) || value instanceof Error) {
            return '';
        }

        const keys = Object.keys(value);
        if (keys.length !== 1) {
            return '';
        }

        const variant = keys[0];
        // @ts-ignore - dynamic object indexing by contract.
        const payload = value[variant];
        const nested = extractErrorText(payload, depth + 1) || String(payload ?? '').trim();
        if (!nested) {
            return '';
        }

        switch (variant) {
            case 'BadRequest':
                return `Bad request: ${nested}`;
            case 'NotFound':
                return `Not found: ${nested}`;
            case 'Unauthorized':
                return `Unauthorized: ${nested}`;
            case 'InternalServerError':
                return `Internal server error: ${nested}`;
            case 'TooManyRequests':
                return nested;
            default:
                return '';
        }
    }

    /**
     * @param {unknown} error
     * @param {string} fallback
     */
    function normalizeInvokeErrorMessage(error, fallback) {
        const extracted = extractErrorText(error);
        if (extracted && extracted !== '[object Object]') {
            return extracted;
        }

        try {
            const serialized = JSON.stringify(error);
            if (serialized && serialized !== '{}') {
                return serialized;
            }
        } catch {
            // Ignore serialization failure and continue fallback chain.
        }

        const stringified = String(error || '').trim();
        if (stringified && stringified !== '[object Object]') {
            return stringified;
        }

        return fallback;
    }

    /**
     * @param {TauriInvokeCommand} command
     * @param {any} args
     */
    async function invokeTransport(command, args = {}) {
        const invokeArgs = withTauriArgumentAliases(args);

        for (let attempt = 0; attempt < 20; attempt += 1) {
            try {
                return await invoke(command, invokeArgs);
            } catch (error) {
                const message = normalizeInvokeErrorMessage(error, `Command failed: ${command}`);
                if (attempt < 19 && shouldRetryInvoke(message)) {
                    await sleep(200);
                    continue;
                }

                const raised = new Error(message);
                // @ts-ignore - assign error cause for better debugging.
                raised.cause = error;
                throw raised;
            }
        }
    }

    let invokeTransportRef = invokeTransport;

    /** @param {string} command @param {any} args */
    const transport = (command, args) => invokeTransportRef(/** @type {TauriInvokeCommand} */ (command), args);

    const invokeBroker = createInvokeBroker({
        transport,
        policies,
    });

    /**
     * @param {TauriInvokeCommand} command
     * @param {any} args
     */
    async function safeInvoke(command, args = {}) {
        return invokeBroker.invoke(command, args);
    }

    /**
     * @param {TauriInvokeCommand} command
     * @param {any} args
     */
    function invalidateInvoke(command, args = {}) {
        invokeBroker.invalidate(command, args);
    }

    /** @param {TauriInvokeCommand} command */
    function invalidateInvokeAll(command) {
        invokeBroker.invalidateAll(command);
    }

    /** @param {TauriInvokeCommand} command */
    function flushInvokes(command) {
        return invokeBroker.flush(command);
    }

    function flushAllInvokes() {
        return invokeBroker.flushAll();
    }

    function installFlushOnHide() {
        const flush = () => flushAllInvokes();

        window.addEventListener('pagehide', flush);
        window.addEventListener('beforeunload', flush);
        document.addEventListener('visibilitychange', () => {
            if (document.visibilityState === 'hidden') {
                void flush();
            }
        });
    }

    return {
        safeInvoke,
        invalidateInvoke,
        invalidateInvokeAll,
        flushInvokes,
        flushAllInvokes,
        get invokeTransport() {
            return invokeTransportRef;
        },
        set invokeTransport(next) {
            invokeTransportRef = next;
        },
        invokeBroker,
        installFlushOnHide,
    };
}
