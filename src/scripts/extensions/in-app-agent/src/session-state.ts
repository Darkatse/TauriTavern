export type AssistantRun = {
    runId: string;
    active: boolean;
    status: TauriTavernAgentRunStatus | 'interrupted';
};

export function terminalStatus(event: TauriTavernAgentRunEvent): TauriTavernAgentRunStatus | null {
    switch (event.type) {
        case 'run_completed': return 'completed';
        case 'run_partial_success': return 'partial_success';
        case 'run_cancelled': return 'cancelled';
        case 'run_failed': return 'failed';
        default: return null;
    }
}

export function progressStatus(event: TauriTavernAgentRunEvent): TauriTavernAgentRunStatus | null {
    if (event.type !== 'status_changed') return null;
    const payload = event.payload;
    if (typeof payload !== 'object' || payload === null || !('status' in payload) || typeof payload.status !== 'string') {
        throw new Error('in-app-agent: status_changed has no status');
    }
    return payload.status as TauriTavernAgentRunStatus;
}

export function appendedMessageSeq(event: TauriTavernAgentRunEvent): number {
    if (event.type !== 'session_message_appended') return 0;
    const payload = event.payload;
    if (typeof payload !== 'object' || payload === null || !('seq' in payload)
        || typeof payload.seq !== 'number' || !Number.isSafeInteger(payload.seq) || payload.seq < 1) {
        throw new Error('in-app-agent: session_message_appended has no valid seq');
    }
    return payload.seq;
}

export function mergeMessages(
    current: TauriTavernAgentSessionMessage[],
    incoming: TauriTavernAgentSessionMessage[],
): TauriTavernAgentSessionMessage[] {
    const bySeq = new Map(current.map(message => [message.seq, message]));
    for (const message of incoming) bySeq.set(message.seq, message);
    return [...bySeq.values()].sort((a, b) => a.seq - b.seq);
}

// The Channel has one current model attempt per invocation. The journal and
// history may arrive first or last; only canonical origin decides replacement.
export function updateResponses(
    responses: Map<string, TauriTavernAgentRunLiveResponse>,
    update: TauriTavernAgentRunLiveUpdate,
): void {
    switch (update.type) {
        case 'snapshot':
            responses.clear();
            for (const response of update.responses) responses.set(response.invocationId, response);
            break;
        case 'responseReplace':
            responses.set(update.response.invocationId, update.response);
            break;
        case 'responseAppend': {
            const response = responses.get(update.invocationId);
            if (!response) throw new Error('in-app-agent: response append has no current attempt');
            responses.set(update.invocationId, {
                ...response,
                text: response.text + update.text,
                reasoning: response.reasoning + update.reasoning,
                toolIds: [...response.toolIds, ...update.toolIds],
            });
            break;
        }
        case 'responseRemove':
            responses.delete(update.invocationId);
            break;
        // Workspace argument previews belong to the writing timeline.
        case 'replace': case 'append': case 'remove': break;
    }
}

export function pendingResponses(
    responses: Map<string, TauriTavernAgentRunLiveResponse>,
    runId: string | undefined,
    messages: TauriTavernAgentSessionMessage[],
): TauriTavernAgentRunLiveResponse[] {
    const saved = new Set(messages.filter(entry => entry.runId === runId && entry.message.role === 'assistant')
        .map(entry => entry.origin && `${entry.origin.invocationId}:${entry.origin.round}`));
    return [...responses.values()].filter(response => response.invocationExitPolicy === 'reply_allowed'
        && !saved.has(`${response.invocationId}:${response.round}`));
}
