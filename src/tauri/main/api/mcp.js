// @ts-check

const SERVER_STATES = new Set(['active', 'paused']);
const TOOL_PERMISSIONS = new Set(['off', 'ask', 'allow']);
const PROTOCOL_VERSIONS = new Set(['auto', '2026-07-28', '2025-11-25', '2025-06-18', '2025-03-26']);

/** @param {unknown} value @param {string} label */
function requireObject(value, label) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(`${label} must be an object`);
    }
    return /** @type {Record<string, any>} */ (value);
}

/** @param {unknown} value @param {string} label */
function requireString(value, label) {
    const text = String(value ?? '').trim();
    if (!text) {
        throw new Error(`${label} is required`);
    }
    return text;
}

/** @param {unknown} value */
function requireNativeName(value) {
    if (typeof value !== 'string' || value.length === 0) {
        throw new Error('nativeName is required');
    }
    return value;
}

/** @param {unknown} value */
function requireArgumentsJson(value) {
    if (typeof value !== 'string') {
        throw new Error('argumentsJson must be a string');
    }
    return value;
}

/** @param {unknown} value */
function headers(value) {
    const headers = requireObject(value, 'headers');
    return Object.fromEntries(Object.entries(headers).map(([name, headerValue]) => {
        if (typeof headerValue !== 'string') {
            throw new Error(`headers.${name} must be a string`);
        }
        return [name, headerValue];
    }));
}

/** @param {unknown} value */
function protocolVersion(value) {
    const version = requireString(value, 'protocolVersion');
    if (!PROTOCOL_VERSIONS.has(version)) {
        throw new Error('protocolVersion is not supported');
    }
    return version;
}

function cancelledBeforeSend() {
    return {
        outcome: 'not_sent',
        code: 'mcp.call_cancelled_before_send',
        message: 'The tool request was cancelled before it started',
    };
}

/** @param {unknown} input */
function registrationId(input) {
    if (typeof input === 'string') {
        return requireString(input, 'registrationId');
    }
    return requireString(requireObject(input, 'input').registrationId, 'registrationId');
}

/** @param {{ safeInvoke: (command: string, args?: any) => Promise<any> }} deps */
function createMcpApi({ safeInvoke }) {
    return {
        servers: {
            list: async () => safeInvoke('list_mcp_servers'),
            create: async (input) => {
                const value = requireObject(input, 'input');
                return safeInvoke('create_mcp_server', {
                    dto: {
                        displayName: requireString(value.displayName, 'displayName'),
                        endpoint: requireString(value.endpoint, 'endpoint'),
                        headers: headers(value.headers ?? {}),
                        protocolVersion: protocolVersion(value.protocolVersion ?? 'auto'),
                    },
                });
            },
            update: async (input) => {
                const value = requireObject(input, 'input');
                return safeInvoke('update_mcp_server', {
                    dto: {
                        registrationId: requireString(value.registrationId, 'registrationId'),
                        displayName: requireString(value.displayName, 'displayName'),
                        endpoint: requireString(value.endpoint, 'endpoint'),
                        headers: headers(value.headers),
                        protocolVersion: protocolVersion(value.protocolVersion),
                    },
                });
            },
            setState: async (input) => {
                const value = requireObject(input, 'input');
                const state = requireString(value.state, 'state');
                if (!SERVER_STATES.has(state)) {
                    throw new Error('state must be active or paused');
                }
                return safeInvoke('set_mcp_server_state', {
                    dto: {
                        registrationId: requireString(value.registrationId, 'registrationId'),
                        state,
                    },
                });
            },
            remove: async (input) => safeInvoke('remove_mcp_server', {
                dto: { registrationId: registrationId(input) },
            }),
            discover: async (input) => safeInvoke('discover_mcp_tools', {
                dto: { registrationId: registrationId(input) },
            }),
            refresh: async (input) => safeInvoke('refresh_mcp_tools', {
                dto: { registrationId: registrationId(input) },
            }),
        },
        tools: {
            setPermission: async (input) => {
                const value = requireObject(input, 'input');
                const permission = requireString(value.permission, 'permission');
                if (!TOOL_PERMISSIONS.has(permission)) {
                    throw new Error('permission must be off, ask, or allow');
                }
                return safeInvoke('set_mcp_tool_permission', {
                    dto: {
                        registrationId: requireString(value.registrationId, 'registrationId'),
                        nativeName: requireNativeName(value.nativeName),
                        permission,
                    },
                });
            },
            testCall: async (input, options = {}) => {
                const value = requireObject(input, 'input');
                const signal = options?.signal;
                if (signal?.aborted) {
                    return cancelledBeforeSend();
                }

                const callId = globalThis.crypto.randomUUID();
                const dto = {
                    callId,
                    registrationId: requireString(value.registrationId, 'registrationId'),
                    nativeName: requireNativeName(value.nativeName),
                    argumentsJson: requireArgumentsJson(value.argumentsJson),
                };
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

export const __test = { createMcpApi };
