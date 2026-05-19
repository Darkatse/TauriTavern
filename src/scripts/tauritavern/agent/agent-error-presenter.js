const RUN_FAILURE_PRESENTATIONS = Object.freeze({
    'model.tool_call_required': Object.freeze({
        message: 'The model skipped the Agent tool flow and tried to answer directly. No committed Agent chat output was kept. Try regenerating; if this keeps happening, reduce the context or use a model with stronger tool calling.',
        messageKey: 'agent.error.model_tool_call_required.message',
        summary: 'The model skipped the Agent tool flow; no Agent chat output was kept.',
        summaryKey: 'agent.error.model_tool_call_required.summary',
    }),
    'agent.tool_after_finish': Object.freeze({
        message: 'The model requested more tools after workspace.finish, which breaks the Agent contract. No committed Agent chat output was kept. Try regenerating; if this persists, lower the temperature or pick a model that obeys workspace.finish.',
        messageKey: 'agent.error.tool_after_finish.message',
        summary: 'Model kept calling tools after workspace.finish; no Agent chat output was kept.',
        summaryKey: 'agent.error.tool_after_finish.summary',
    }),
    'agent.max_tool_rounds_exceeded': Object.freeze({
        message: 'The Agent loop exceeded the configured maximum tool rounds before calling workspace.finish. No committed Agent chat output was kept. Try regenerating with a tighter prompt, or raise the round budget in the profile.',
        messageKey: 'agent.error.max_tool_rounds_exceeded.message',
        summary: 'Tool round budget exhausted; no Agent chat output was kept.',
        summaryKey: 'agent.error.max_tool_rounds_exceeded.summary',
    }),
});

export function presentAgentRunFailure(event) {
    const payload = event?.payload || {};
    const code = String(payload.code || '').trim();
    const message = String(payload.message || '').trim();
    const technicalMessage = String(payload.technicalMessage || message || runFailed()).trim();
    const presentation = RUN_FAILURE_PRESENTATIONS[code];
    const retryable = payload.retryable === true;
    // Backend guarantees retryable=true implies userRetryable=true. If the
    // backend omits userRetryable (older runtimes) fall back to retryable so
    // we never block a manual retry that auto-retry would have allowed.
    const userRetryable = payload.userRetryable === true || retryable;

    return {
        code,
        message: presentation
            ? translateAgentError(presentation.message, presentation.messageKey)
            : message || technicalMessage,
        summary: presentation
            ? translateAgentError(presentation.summary, presentation.summaryKey)
            : message || technicalMessage,
        technicalMessage,
        retryable,
        userRetryable,
    };
}

export function agentErrorMessage(error) {
    return String(error?.userMessage || error?.message || error || runFailed());
}

function runFailed() {
    return translateAgentError('Agent run failed', 'agent.error.run_failed');
}

function translateAgentError(message, key) {
    const translate = globalThis.SillyTavern?.getContext?.()?.translate;
    return typeof translate === 'function' ? translate(message, key) : message;
}
