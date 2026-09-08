import { presentAgentRunFailure } from './agent-error-presenter.js';
import { rollbackAgentRunDriftMessages } from './agent-run-message-rollback.js';

const AGENT_RUN_STATE_CHANGED = 'tauritavern-agent-run-state-changed';
const AGENT_RUN_EVENT = 'tauritavern-agent-run-event';
const TERMINAL_EVENTS = new Set(['run_completed', 'run_partial_success', 'run_cancelled', 'run_failed']);
const ROLLBACK_EVENT_TYPE = 'run_rollback_targets';

let activeRun = null;
let startingRun = false;
let rollbackScriptOverride = null;
let guidanceSequence = 0;

function requireAgentApi() {
    const agent = window.__TAURITAVERN__?.api?.agent;
    if (!agent) {
        throw new Error('TauriTavern Agent API is unavailable');
    }
    return agent;
}

function emitRunStateChanged(lastEvent = null, presentationError = '') {
    window.dispatchEvent(new CustomEvent(AGENT_RUN_STATE_CHANGED, {
        detail: {
            activeRun,
            lastEvent,
            presentationError,
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

function errorFromRollbackFailure(error, terminalEvent) {
    const message = error?.message || String(error || 'unknown rollback failure');
    const wrapped = new Error(`Agent drift rollback failed before ${terminalEvent?.type || 'terminal event'}: ${message}`);
    wrapped.name = 'AgentRunRollbackError';
    wrapped.cause = error;
    wrapped.event = terminalEvent;
    wrapped.agentErrorCode = 'agent.rollback_failed';
    wrapped.retryable = false;
    wrapped.userRetryable = false;
    return wrapped;
}

// Lazy-load the SillyTavern vendor module so tests and the agent-system bundle
// can use this controller without pulling the whole chat runtime into their
// module graphs.
async function loadRollbackScript() {
    if (rollbackScriptOverride) {
        return rollbackScriptOverride;
    }
    // Rspack supports webpackIgnore and leaves this host runtime import native.
    return import('/script.js' /* webpackIgnore: true */);
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

export async function retryAgentRunPresentation(runId) {
    await requireAgentApi().settleChatPresentation({ runId });
    emitRunStateChanged();
}

export async function submitGuidanceToActiveAgentRun(text) {
    const runId = String(activeRun?.runId || '').trim();
    if (!runId) {
        throw new Error('agent.guidance_active_run_missing: no active Agent run');
    }

    return requireAgentApi().submitGuidance({
        runId,
        text,
        clientGuidanceId: createClientGuidanceId(),
    });
}

export async function startAndWaitForAgentRun(input) {
    return runAndWait(agent => agent.startRunWithPromptSnapshot(input));
}

export async function resumeAndWaitForAgentRun(input, abortController = null) {
    return runAndWait(agent => agent.resume(input), abortController);
}

async function runAndWait(start, abortController = null) {
    if (abortController?.signal.aborted) return;
    if (activeRun || startingRun) throw new Error('An Agent run is already active');
    const agent = requireAgentApi();
    startingRun = true;
    let handle;
    try {
        handle = await start(agent);
    } catch (error) {
        if (error?.name === 'AbortError') return;
        throw error;
    } finally {
        startingRun = false;
    }
    activeRun = handle;
    emitRunStateChanged();

    return new Promise((resolve, reject) => {
        let stop = () => {};
        // Legacy and explicit-discard flows can surface rollback targets
        // before the matching terminal event. We start cleanup immediately,
        // then wait for it before settling so vendor's finally(unblockGeneration)
        // observes the intended chat state.
        let pendingRollback = Promise.resolve();

        const clearActiveRun = (lastEvent = null, presentationError = '') => {
            abortController?.removeEventListener('abort', cancel);
            activeRun = null;
            emitRunStateChanged(lastEvent, presentationError);
        };
        const onError = error => {
            stop();
            clearActiveRun();
            reject(error);
        };
        const cancel = () => { void agent.cancel({ runId: handle.runId }).catch(onError); };

        try {
            stop = agent.subscribe(handle.runId, (event) => {
                emitRunEvent(event);

                if (event?.type === ROLLBACK_EVENT_TYPE) {
                    pendingRollback = pendingRollback
                        .then(() => handleRollbackEvent(handle.runId, event));
                    // Keep the promise observed; the terminal event still reports the failure.
                    void pendingRollback.catch(() => {});
                    return;
                }

                if (!TERMINAL_EVENTS.has(event?.type)) {
                    return;
                }

                stop();
                const pending = pendingRollback;
                void pending.then(async () => {
                    await agent.settleChatPresentation(handle);
                    clearActiveRun(event);

                    if (event.type === 'run_failed') {
                        reject(errorFromRunEvent(event));
                        return;
                    }

                    resolve({
                        handle,
                        terminalEvent: event,
                    });
                }, (rollbackError) => {
                    clearActiveRun(event);
                    reject(errorFromRollbackFailure(rollbackError, event));
                }).catch((error) => {
                    clearActiveRun(event, String(error?.message ?? error));
                    reject(error);
                });
            }, {
                afterSeq: handle.afterSeq ?? 0,
                onError,
            });
            abortController?.addEventListener('abort', cancel);
            // A slash command can be stopped while the backend is admitting the run.
            if (abortController?.signal.aborted) cancel();
        } catch (error) {
            clearActiveRun();
            reject(error);
        }
    });
}

async function handleRollbackEvent(runId, event) {
    const targets = event?.payload?.targets;
    if (!Array.isArray(targets)) {
        throw new Error('agent.rollback_targets_invalid: run_rollback_targets payload.targets must be an array');
    }
    if (targets.length === 0) {
        return;
    }
    const script = await loadRollbackScript();
    await rollbackAgentRunDriftMessages({ runId, targets, script });
}

function createClientGuidanceId() {
    const randomId = globalThis.crypto?.randomUUID?.();
    if (randomId) {
        return `client_guidance_${randomId}`;
    }

    guidanceSequence += 1;
    return `client_guidance_${Date.now()}_${guidanceSequence}`;
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
