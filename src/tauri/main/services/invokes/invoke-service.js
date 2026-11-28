// @ts-check

import { extractErrorText } from '../../../../scripts/util/command-error-utils.js';
import { createInvokeBroker } from '../../brokers/invoke-broker.js';
import { findUpstreamFailureDetails } from '../../kernel/upstream-failure.js';

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
    /** @param {Record<string, any> | null | undefined} args */
    function withTauriArgumentAliases(args) {
        if (!args || typeof args !== 'object' || Array.isArray(args)
            || ArrayBuffer.isView(args) || args instanceof ArrayBuffer) {
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

    /**
     * @param {TauriInvokeCommand} command
     * @param {any} args
     * @param {{ headers?: HeadersInit }} [options]
     */
    async function invokeTransport(command, args = {}, options) {
        const invokeArgs = withTauriArgumentAliases(args);

        try {
            return await invoke(command, invokeArgs, options);
        } catch (error) {
            const raised = new Error(extractErrorText(error) || `Command failed: ${command}`, { cause: error });
            const details = findUpstreamFailureDetails(error);
            if (details) {
                // @ts-ignore - structured backend error details.
                raised.details = details;
            }
            throw raised;
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
    };
}
