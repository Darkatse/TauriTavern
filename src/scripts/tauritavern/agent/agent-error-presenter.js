const RUN_FAILURE_PRESENTATIONS = Object.freeze({
    'model.tool_call_required': Object.freeze({
        message: 'The model skipped the Agent tool flow and tried to answer directly. No chat message was committed. Try regenerating; if this keeps happening, reduce the context or use a model with stronger tool calling.',
        messageKey: 'agent.error.model_tool_call_required.message',
        summary: 'The model skipped the Agent tool flow; no message was committed.',
        summaryKey: 'agent.error.model_tool_call_required.summary',
    }),
});

export function presentAgentRunFailure(event) {
    const payload = event?.payload || {};
    const code = String(payload.code || '').trim();
    const message = String(payload.message || '').trim();
    const technicalMessage = String(payload.technicalMessage || message || runFailed()).trim();
    const presentation = RUN_FAILURE_PRESENTATIONS[code];

    return {
        code,
        message: presentation
            ? translateAgentError(presentation.message, presentation.messageKey)
            : message || technicalMessage,
        summary: presentation
            ? translateAgentError(presentation.summary, presentation.summaryKey)
            : message || technicalMessage,
        technicalMessage,
        retryable: payload.retryable === true,
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
