// @ts-check

// The Rust command DTOs own input validation; this module only maps the Host ABI onto commands.

function cancelledBeforeSend() {
    return {
        outcome: 'not_sent',
        code: 'mcp.call_cancelled_before_send',
        message: 'The tool request was cancelled before it started',
    };
}

/** @param {string | { registrationId: string }} input */
function registrationDto(input) {
    return { registrationId: typeof input === 'string' ? input : input?.registrationId };
}

/** @param {{ safeInvoke: (command: string, args?: any) => Promise<any> }} deps */
function createMcpApi({ safeInvoke }) {
    return {
        servers: {
            list: async () => safeInvoke('list_mcp_servers'),
            create: async (input) => safeInvoke('create_mcp_server', { dto: input }),
            update: async (input) => safeInvoke('update_mcp_server', { dto: input }),
            setState: async (input) => safeInvoke('set_mcp_server_state', { dto: input }),
            remove: async (input) => safeInvoke('remove_mcp_server', { dto: registrationDto(input) }),
            discover: async (input) => safeInvoke('discover_mcp_tools', { dto: registrationDto(input) }),
            refresh: async (input) => safeInvoke('refresh_mcp_tools', { dto: registrationDto(input) }),
        },
        tools: {
            setPermission: async (input) => safeInvoke('set_mcp_tool_permission', { dto: input }),
            setDescriptionOverride: async (input) => safeInvoke('set_mcp_tool_description_override', { dto: input }),
            testCall: async (input, options = {}) => {
                const signal = options?.signal;
                if (signal?.aborted) {
                    return cancelledBeforeSend();
                }

                const callId = globalThis.crypto.randomUUID();
                // Bind the request at call time: the start acknowledgement below yields first.
                const dto = { ...input, callId };
                const cancel = () => {
                    void safeInvoke('cancel_mcp_test_call', { dto: { callId } })
                        .catch(error => console.debug('Failed to stop MCP test call:', error));
                };

                // The acknowledgement closes the cancel-before-register race without
                // retaining cancellation tombstones in the backend.
                await safeInvoke('start_mcp_test_call', { dto: { callId } });
                if (signal?.aborted) {
                    cancel();
                    return cancelledBeforeSend();
                }

                let abortHandler = null;
                if (signal) {
                    abortHandler = cancel;
                    signal.addEventListener('abort', abortHandler, { once: true });
                }

                try {
                    return await safeInvoke('test_mcp_tool_call', { dto });
                } catch (error) {
                    // Also releases the start acknowledgement when the backend rejects the input.
                    cancel();
                    throw error;
                } finally {
                    if (signal && abortHandler) {
                        signal.removeEventListener('abort', abortHandler);
                    }
                }
            },
        },
    };
}

/** @param {any} context */
export function installMcpApi(context) {
    const hostAbi = window.__TAURITAVERN__;
    if (!hostAbi || typeof hostAbi !== 'object') {
        throw new Error('Host ABI __TAURITAVERN__ is missing');
    }
    if (typeof context?.safeInvoke !== 'function') {
        throw new Error('Tauri main context safeInvoke is missing');
    }
    if (!hostAbi.api || typeof hostAbi.api !== 'object') {
        hostAbi.api = {};
    }
    hostAbi.api.mcp = createMcpApi({ safeInvoke: context.safeInvoke });
}
