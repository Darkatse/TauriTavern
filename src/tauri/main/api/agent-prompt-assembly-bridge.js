// @ts-check

/** @typedef {(command: string, args?: any) => Promise<any>} SafeInvoke */
/** @typedef {{ runId: string; resolvedAssemblyIds: Set<string>; stop: (() => void) | null }} AssemblyBridgeState */

/** @type {Map<string, AssemblyBridgeState>} */
const activePromptAssemblyBridges = new Map();
const TERMINAL_EVENTS = new Set(['run_completed', 'run_partial_success', 'run_cancelled', 'run_failed']);

/** @param {{ runId: string; safeInvoke: SafeInvoke; promptAssembly: TauriTavernAgentPromptAssemblyApi; subscribe: TauriTavernAgentApi['subscribe'] }} deps */
export function attachHostPromptAssemblyBridge({ runId, safeInvoke, promptAssembly, subscribe }) {
    const normalizedRunId = requireRunId(runId);
    if (activePromptAssemblyBridges.has(normalizedRunId)) {
        return activePromptAssemblyBridges.get(normalizedRunId);
    }
    if (typeof promptAssembly?.buildSnapshot !== 'function') {
        throw new Error('agent.prompt_assembly_api_unavailable: promptAssembly.buildSnapshot is required');
    }

    /** @type {AssemblyBridgeState} */
    const state = {
        runId: normalizedRunId,
        resolvedAssemblyIds: new Set(),
        stop: null,
    };
    const stop = subscribe(normalizedRunId, (event) => {
        if (event?.type === 'prompt_assembly_requested') {
            void handlePromptAssemblyRequested({
                state,
                event,
                safeInvoke,
                promptAssembly,
            }).catch((error) => {
                queueMicrotask(() => {
                    throw error;
                });
            });
            return;
        }

        if (TERMINAL_EVENTS.has(event?.type)) {
            detachHostPromptAssemblyBridge(normalizedRunId);
        }
    }, {
        onError(error) {
            queueMicrotask(() => {
                throw error;
            });
        },
    });

    state.stop = stop;
    activePromptAssemblyBridges.set(normalizedRunId, state);
    return state;
}

/** @param {string} runId */
function detachHostPromptAssemblyBridge(runId) {
    const normalizedRunId = requireRunId(runId);
    const state = activePromptAssemblyBridges.get(normalizedRunId);
    if (!state) {
        return;
    }
    activePromptAssemblyBridges.delete(normalizedRunId);
    if (typeof state.stop === 'function') {
        state.stop();
    }
}

/** @param {{ state: AssemblyBridgeState; event: TauriTavernAgentRunEvent; safeInvoke: SafeInvoke; promptAssembly: TauriTavernAgentPromptAssemblyApi }} input */
async function handlePromptAssemblyRequested({ state, event, safeInvoke, promptAssembly }) {
    const payload = event?.payload || {};
    const assemblyId = requirePayloadString(payload, 'assemblyId');
    if (state.resolvedAssemblyIds.has(assemblyId)) {
        return;
    }
    state.resolvedAssemblyIds.add(assemblyId);

    try {
        const request = await readPromptAssemblyRequest({ safeInvoke, runId: state.runId, assemblyId });
        if (request === null) return;
        const assembled = await promptAssembly.buildSnapshot(request);
        await safeInvoke('resolve_agent_prompt_assembly', {
            dto: {
                runId: state.runId,
                assemblyId,
                promptSnapshot: assembled.promptSnapshot,
                frozenRunInputSnapshot: assembled.frozenRunInputSnapshot,
                generationIntent: assembled.generationIntent,
                assembly: assembled.assembly,
            },
        });
    } catch (error) {
        await safeInvoke('resolve_agent_prompt_assembly', {
            dto: {
                runId: state.runId,
                assemblyId,
                error: String(/** @type {any} */ (error)?.message ?? error),
            },
        });
    }
}

/** @param {{ safeInvoke: SafeInvoke; runId: string; assemblyId: string }} input */
async function readPromptAssemblyRequest({ safeInvoke, runId, assemblyId }) {
    const request = await safeInvoke('read_agent_prompt_assembly_request', {
        dto: {
            runId,
            assemblyId,
        },
    });
    if (request === null) return null;
    return requirePlainObject(
        request,
        'agent.prompt_assembly_request_invalid: pending prompt assembly request must be an object',
    );
}

/** @param {unknown} value */
function requireRunId(value) {
    const runId = String(value || '').trim();
    if (!runId) {
        throw new Error('runId is required');
    }
    return runId;
}

/** @param {any} payload @param {string} field */
function requirePayloadString(payload, field) {
    const value = String(payload?.[field] || '').trim();
    if (!value) {
        throw new Error(`${field} is required`);
    }
    return value;
}

/** @param {any} value @param {string} message @returns {any} */
function requirePlainObject(value, message) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(message);
    }
    return value;
}
