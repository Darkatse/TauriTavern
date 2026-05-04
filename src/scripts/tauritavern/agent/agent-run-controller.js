const AGENT_RUN_STATE_CHANGED = 'tauritavern-agent-run-state-changed';
const AGENT_RUN_EVENT = 'tauritavern-agent-run-event';
const TERMINAL_EVENTS = new Set(['run_completed', 'run_cancelled', 'run_failed']);

let activeRun = null;

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
    const message = event?.payload?.message || 'Agent run failed';
    const error = new Error(message);
    error.event = event;
    return error;
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

        const clearActiveRun = (lastEvent = null) => {
            activeRun = null;
            emitRunStateChanged(lastEvent);
        };

        try {
            stop = agent.subscribe(handle.runId, (event) => {
                emitRunEvent(event);

                if (!TERMINAL_EVENTS.has(event?.type)) {
                    return;
                }

                stop();
                clearActiveRun(event);

                if (event.type === 'run_failed') {
                    reject(errorFromRunEvent(event));
                    return;
                }

                resolve({
                    handle,
                    terminalEvent: event,
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
