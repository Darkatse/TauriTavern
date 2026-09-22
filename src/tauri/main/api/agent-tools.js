// @ts-check

import { createChannel } from '../../../tauri-bridge.js';

/**
 * @param {{
 *   safeInvoke: (command: string, args?: any) => Promise<any>;
 *   channelFactory?: (onmessage: (event: any) => void) => any;
 * }} deps
 * @returns {TauriTavernAgentToolsApi}
 */
export function createAgentToolsApi({ safeInvoke, channelFactory = createChannel }) {
    return {
        list(options) {
            return options === undefined
                ? safeInvoke('list_agent_tools')
                : safeInvoke('list_agent_tools', { dto: options });
        },
        setEnabled(toolId, enabled) {
            return safeInvoke('set_agent_extension_tool_enabled', { dto: { toolId, enabled } });
        },
        async register(definition, execute) {
            if (typeof execute !== 'function') throw new TypeError('An extension tool execute function is required');
            /** @type {Map<string, AbortController>} */
            const pending = new Map();

            /** @param {{ requestId: string; call: Omit<TauriTavernExtensionToolContext, 'signal'> & { arguments: Record<string, TauriTavernJsonValue> } }} event */
            async function executeCall({ requestId, call }) {
                const controller = new AbortController();
                pending.set(requestId, controller);
                try {
                    const value = await execute(call.arguments, {
                        runId: call.runId,
                        invocationId: call.invocationId,
                        callId: call.callId,
                        target: call.target,
                        signal: controller.signal,
                    });
                    if (!pending.has(requestId)) return;
                    await safeInvoke('resolve_agent_extension_tool_call', {
                        dto: { requestId, result: { kind: 'value', value: value === undefined ? null : value } },
                    });
                } catch (error) {
                    if (!pending.has(requestId)) return;
                    await safeInvoke('resolve_agent_extension_tool_call', {
                        dto: { requestId, result: { kind: 'error', message: error instanceof Error ? error.message : String(error) } },
                    });
                } finally {
                    pending.delete(requestId);
                }
            }

            const channel = channelFactory(event => {
                if (event.type === 'cancel') {
                    const controller = pending.get(event.requestId);
                    pending.delete(event.requestId);
                    controller?.abort();
                    return;
                }
                return executeCall(event).catch(error => {
                    console.error('[AgentTools] Failed to submit extension tool result', error);
                });
            });
            await safeInvoke('register_agent_extension_tool', { definition, channel });
        },
    };
}
