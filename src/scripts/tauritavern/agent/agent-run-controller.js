import { presentAgentRunFailure } from './agent-error-presenter.js';
import { rollbackAgentRunDriftMessages } from './agent-run-message-rollback.js';

const AGENT_RUN_STATE_CHANGED = 'tauritavern-agent-run-state-changed';
const AGENT_RUN_EVENT = 'tauritavern-agent-run-event';
const TERMINAL_EVENTS = new Set(['run_completed', 'run_cancelled', 'run_failed']);
const ROLLBACK_EVENT_TYPE = 'run_rollback_targets';

let activeRun = null;
let rollbackScriptOverride = null;

function requireAgentApi() {
    const agent = window.__TAURITAVERN__?.api?.agent;
    if (!agent) {
        throw new Error('TauriTavern Agent API is unavailable');
    }
    return agent;
}

function emitRunStateChanged(lastEvent = null) {
    window.dispatchEvent(new CustomEvent(AGENT_RUN_STATE_CHANGED, {
        detail: {
            activeRun,
            lastEvent,
        },
    }));
}

function emitRunEvent(event) {
    window.dispatchEvent(new CustomEvent(AGENT_RUN_EVENT, {
        detail: { event },
    }));
}

function errorFromRunEvent(event) {
    const presentation = presentAgentRunFailure(event);
    const error = new Error(presentation.message);
    error.name = 'AgentRunError';
    error.event = event;
    error.agentErrorCode = presentation.code;
    error.userMessage = presentation.message;
    error.technicalMessage = presentation.technicalMessage;
    error.retryable = presentation.retryable;
    error.userRetryable = presentation.userRetryable;
    return error;
}

// Lazy-load the SillyTavern vendor module so tests can inject a fake script
// surface via __setAgentRunRollbackScriptForTests without dragging the whole
// chat runtime into the unit-test sandbox.
async function loadRollbackScript() {
    if (rollbackScriptOverride) {
        return rollbackScriptOverride;
    }
    return import('../../../script.js');
}

export function __setAgentRunRollbackScriptForTests(script) {
    rollbackScriptOverride = script;
}

export function getActiveAgentRun() {
    return activeRun;
}

export function hasActiveAgentRun() {
    return Boolean(activeRun?.runId);
}

export async function cancelActiveAgentRun() {
    if (!activeRun?.runId) {
        return false;
    }

    await requireAgentApi().cancel(activeRun.runId);
    return true;
}

export async function startAndWaitForAgentRun(input) {
    if (activeRun?.runId) {
        throw new Error(`Agent run ${activeRun.runId} is already active`);
    }

    const agent = requireAgentApi();
    const handle = await agent.startRunWithPromptSnapshot(input);
    activeRun = handle;
    emitRunStateChanged();

    return new Promise((resolve, reject) => {
        let stop = () => {};
        // Drift artifacts surface on `run_rollback_targets` (level=Warn)
        // strictly before the matching `run_failed`. We start the rollback
        // immediately on the first event but only resolve the run promise
        // once it settles so vendor's finally(unblockGeneration) fires on
        // a chat that already matches the failed run's state.
        let pendingRollback = Promise.resolve();

        const clearActiveRun = (lastEvent = null) => {
            activeRun = null;
            emitRunStateChanged(lastEvent);
        };

        try {
            stop = agent.subscribe(handle.runId, (event) => {
                emitRunEvent(event);

                if (event?.type === ROLLBACK_EVENT_TYPE) {
                    pendingRollback = pendingRollback
                        .then(() => handleRollbackEvent(handle.runId, event))
                        .catch((rollbackError) => {
                            console.error('[TauriTavern] Agent drift rollback failed', rollbackError);
                        });
                    return;
                }

                if (!TERMINAL_EVENTS.has(event?.type)) {
                    return;
                }

                stop();
                const pending = pendingRollback;
                void pending.finally(() => {
                    clearActiveRun(event);

                    if (event.type === 'run_failed') {
                        reject(errorFromRunEvent(event));
                        return;
                    }

                    resolve({
                        handle,
                        terminalEvent: event,
                    });
                });
            }, {
                onError(error) {
                    stop();
                    clearActiveRun();
                    reject(error);
                },
            });
        } catch (error) {
            clearActiveRun();
            reject(error);
        }
    });
}

async function handleRollbackEvent(runId, event) {
    const targets = Array.isArray(event?.payload?.targets) ? event.payload.targets : [];
    if (targets.length === 0) {
        return;
    }
    const script = await loadRollbackScript();
    await rollbackAgentRunDriftMessages({ runId, targets, script });
}

export function subscribeAgentRunState(listener) {
    const handler = (event) => listener(event.detail);
    window.addEventListener(AGENT_RUN_STATE_CHANGED, handler);
    return () => window.removeEventListener(AGENT_RUN_STATE_CHANGED, handler);
}

export function subscribeAgentRunEvents(listener) {
    const handler = (event) => listener(event.detail.event);
    window.addEventListener(AGENT_RUN_EVENT, handler);
    return () => window.removeEventListener(AGENT_RUN_EVENT, handler);
}
