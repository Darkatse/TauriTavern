// @ts-check

const activePromptAssemblyBridges = new Map();
const TERMINAL_EVENTS = new Set(['run_completed', 'run_partial_success', 'run_cancelled', 'run_failed']);

export function attachHostPromptAssemblyBridge({ runId, safeInvoke, promptAssembly, subscribe }) {
    const normalizedRunId = requireRunId(runId);
    if (activePromptAssemblyBridges.has(normalizedRunId)) {
        return activePromptAssemblyBridges.get(normalizedRunId);
    }
    if (typeof promptAssembly?.buildSnapshot !== 'function') {
        throw new Error('agent.prompt_assembly_api_unavailable: promptAssembly.buildSnapshot is required');
    }

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

async function handlePromptAssemblyRequested({ state, event, safeInvoke, promptAssembly }) {
    const payload = event?.payload || {};
    const assemblyId = requirePayloadString(payload, 'assemblyId');
    if (state.resolvedAssemblyIds.has(assemblyId)) {
        return;
    }
    state.resolvedAssemblyIds.add(assemblyId);

    try {
        const request = await readPromptAssemblyRequest({
            safeInvoke,
            runId: state.runId,
            assemblyId,
        });
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
                error: String(error?.message ?? error),
            },
        });
    }
}

async function readPromptAssemblyRequest({ safeInvoke, runId, assemblyId }) {
    const request = await safeInvoke('read_agent_prompt_assembly_request', {
        dto: {
            runId,
            assemblyId,
        },
    });
    return requirePlainObject(
        request,
        'agent.prompt_assembly_request_invalid: pending prompt assembly request must be an object',
    );
}

function requireRunId(value) {
    const runId = String(value || '').trim();
    if (!runId) {
        throw new Error('runId is required');
    }
    return runId;
}

function requirePayloadString(payload, field) {
    const value = String(payload?.[field] || '').trim();
    if (!value) {
        throw new Error(`${field} is required`);
    }
    return value;
}

function requirePlainObject(value, message) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(message);
    }
    return value;
}
