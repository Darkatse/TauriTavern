import { presentAgentRunFailure } from '../../../tauritavern/agent/agent-error-presenter.js';
import type { AgentSystemMessageKey, AgentSystemMessageParams } from './i18n';
import { displayToolName } from './run-tool-labels';
import { textMetricsSummary } from './run-text-metrics';
import type { TimelineItem } from './RunTimelineContract';

type RunEventPayload = Record<string, unknown>;
type EventMeta = {
    icon: string;
    tone: string;
    kind?: string;
    titleKey: AgentSystemMessageKey;
};

const NARRATION_EXPANDED_CHAR_THRESHOLD = 36;
const NARRATION_EXPANDED_ROW_SPAN = 2;
export const HANDOFF_EVENT_META: EventMeta = Object.freeze({
    icon: 'fa-arrow-right-arrow-left',
    tone: 'active',
    kind: 'handoff',
    titleKey: 'timelineEventHandoffAccepted',
});

const EVENT_META: Readonly<Record<string, EventMeta>> = Object.freeze({
    agent_delegate_started: { icon: 'fa-diagram-project', tone: 'active', kind: 'subagent', titleKey: 'timelineEventSubAgentStarted' },
    agent_handoff_accepted: HANDOFF_EVENT_META,
    agent_invocation_started: { icon: 'fa-circle-play', tone: 'active', kind: 'subagent', titleKey: 'timelineEventInvocationStarted' },
    agent_invocation_completed: { icon: 'fa-circle-check', tone: 'success', kind: 'subagent', titleKey: 'timelineEventInvocationCompleted' },
    agent_invocation_failed: { icon: 'fa-circle-exclamation', tone: 'error', kind: 'subagent', titleKey: 'timelineEventInvocationFailed' },
    agent_invocation_cancelled: { icon: 'fa-ban', tone: 'warn', kind: 'subagent', titleKey: 'timelineEventInvocationCancelled' },
    agent_task_started: { icon: 'fa-person-running', tone: 'active', kind: 'subagent', titleKey: 'timelineEventSubAgentTaskStarted' },
    agent_task_completed: { icon: 'fa-circle-check', tone: 'success', kind: 'subagent', titleKey: 'timelineEventSubAgentTaskCompleted' },
    agent_task_failed: { icon: 'fa-triangle-exclamation', tone: 'error', kind: 'subagent', titleKey: 'timelineEventSubAgentTaskFailed' },
    agent_task_cancelled: { icon: 'fa-ban', tone: 'warn', kind: 'subagent', titleKey: 'timelineEventSubAgentTaskCancelled' },
    context_assembled: { icon: 'fa-triangle-exclamation', tone: 'warn', kind: 'tool', titleKey: 'timelineEventToolConfigurationWarning' },
    task_return_completed: { icon: 'fa-reply', tone: 'success', kind: 'subagent', titleKey: 'timelineEventTaskReturned' },
    tool_call_requested: { icon: 'fa-screwdriver-wrench', tone: 'active', kind: 'tool', titleKey: 'timelineEventToolRequested' },
    tool_call_completed: { icon: 'fa-check', tone: 'success', kind: 'tool', titleKey: 'timelineEventToolCompleted' },
    tool_call_failed: { icon: 'fa-triangle-exclamation', tone: 'warn', kind: 'fail', titleKey: 'timelineEventToolFailed' },
    workspace_file_written: { icon: 'fa-file-lines', tone: 'success', kind: 'write', titleKey: 'timelineEventFileWritten' },
    direct_output_captured: { icon: 'fa-file-lines', tone: 'warn', kind: 'recover', titleKey: 'timelineEventDirectOutputCaptured' },
    workspace_patch_applied: { icon: 'fa-code-commit', tone: 'success', kind: 'patch', titleKey: 'timelineEventPatchApplied' },
    chat_commit_requested: { icon: 'fa-message', tone: 'active', kind: 'commit', titleKey: 'timelineEventCommitRequested' },
    chat_commit_completed: { icon: 'fa-circle-check', tone: 'success', kind: 'commit', titleKey: 'timelineEventCommitCompleted' },
    chat_commit_failed: { icon: 'fa-circle-exclamation', tone: 'warn', kind: 'fail', titleKey: 'timelineEventCommitFailed' },
    persistent_changes_committed: { icon: 'fa-database', tone: 'success', kind: 'persist', titleKey: 'timelineEventPersistentCommitted' },
    drift_recovery_attempted: { icon: 'fa-arrows-rotate', tone: 'warn', kind: 'recover', titleKey: 'timelineEventDriftRecoveryAttempted' },
    user_guidance_submitted: { icon: 'fa-user-pen', tone: 'active', kind: 'guidance', titleKey: 'timelineEventGuidanceSubmitted' },
    user_guidance_applied: { icon: 'fa-share', tone: 'success', kind: 'guidance', titleKey: 'timelineEventGuidanceApplied' },
    user_guidance_discarded: { icon: 'fa-ban', tone: 'warn', kind: 'guidance', titleKey: 'timelineEventGuidanceDiscarded' },
    model_completed: { icon: 'fa-quote-left', tone: 'info', kind: 'narration', titleKey: 'timelineEventNarration' },
    run_completed: { icon: 'fa-circle-check', tone: 'success', kind: 'done', titleKey: 'timelineEventRunCompleted' },
    run_partial_success: { icon: 'fa-circle-exclamation', tone: 'warn', kind: 'partial', titleKey: 'timelineEventRunPartialSuccess' },
    run_cancelled: { icon: 'fa-ban', tone: 'warn', kind: 'cancel', titleKey: 'timelineEventRunCancelled' },
    run_failed: { icon: 'fa-circle-xmark', tone: 'error', kind: 'fail', titleKey: 'timelineEventRunFailed' },
});

export function presentRunEvent(
    event: TauriTavernAgentRunEvent,
    allEvents: readonly TauriTavernAgentRunEvent[] = [],
): TimelineItem {
    const type = event.type;
    const payload = plainObject(event.payload) ? event.payload : {};
    const meta = EVENT_META[type] ?? {
        icon: 'fa-circle',
        tone: event.level === 'error' ? 'error' : 'info',
        titleKey: 'timelineEventGeneric',
    };
    const rowSpan = eventRowSpan(type, payload);

    return {
        id: event.id || `${event.runId || 'run'}:${event.seq || type}`,
        seq: Number(event.seq || 0),
        runId: event.runId,
        type,
        level: event.level || 'info',
        timestamp: event.timestamp || '',
        icon: meta.icon,
        tone: event.level === 'error' ? 'error' : meta.tone,
        kind: eventKind(type, payload, meta.kind),
        titleKey: meta.titleKey,
        titleParams: eventTitleParams(type, payload),
        summary: eventSummary(type, payload, allEvents),
        rawEvent: event,
        ...(rowSpan > 1 ? { rowSpan } : {}),
    };
}

export function modelTurnNarration(payload: unknown): string {
    const value = plainObject(payload) ? payload : {};
    const narration = plainObject(value.narration) ? value.narration : null;
    return stringValue(narration?.text).trim();
}

function eventRowSpan(type: string, payload: RunEventPayload): number {
    if (type !== 'model_completed') {
        return 1;
    }
    const narration = modelTurnNarration(payload);
    if (!narration) {
        return 1;
    }
    const narrationValue = plainObject(payload.narration) ? payload.narration : {};
    const totalChars = Number(narrationValue.totalChars);
    const length = Number.isFinite(totalChars) && totalChars > 0
        ? totalChars
        : narration.length;
    return length > NARRATION_EXPANDED_CHAR_THRESHOLD ? NARRATION_EXPANDED_ROW_SPAN : 1;
}

function eventTitleParams(type: string, payload: RunEventPayload): AgentSystemMessageParams {
    switch (type) {
        case 'model_completed':
            return { text: modelTurnNarration(payload) };
        case 'agent_handoff_accepted':
            return { agent: firstString(payload.targetProfileId, payload.newInvocationId) };
        case 'agent_delegate_started':
        case 'agent_task_started':
        case 'agent_task_completed':
        case 'agent_task_failed':
        case 'agent_task_cancelled':
            return { agent: firstString(payload.targetProfileId, payload.workspaceKey, payload.childInvocationId) };
        case 'agent_invocation_started':
        case 'agent_invocation_completed':
        case 'agent_invocation_failed':
        case 'agent_invocation_cancelled':
            return { agent: firstString(payload.profileId, payload.invocationId) };
        case 'task_return_completed':
            return { task: stringValue(payload.taskId) };
        case 'context_assembled':
            return { count: Array.isArray(payload.toolDiagnostics) ? payload.toolDiagnostics.length : 0 };
        case 'tool_call_requested':
        case 'tool_call_completed':
        case 'tool_call_failed':
            return { tool: displayToolName(payload.name) };
        case 'workspace_file_written':
        case 'direct_output_captured':
        case 'workspace_patch_applied':
        case 'chat_commit_requested':
        case 'chat_commit_completed':
            return { path: stringValue(payload.path) };
        case 'persistent_changes_committed':
            return { count: messageValue(payload.changeCount, 0) };
        case 'drift_recovery_attempted':
            return {
                attempt: messageValue(payload.attempt, 0),
                max: messageValue(payload.maxAttempts, 0),
            };
        case 'user_guidance_applied':
        case 'user_guidance_discarded':
            return { count: messageValue(payload.count, normalizeGuidanceIds(payload).length) };
        case 'run_partial_success':
            return { count: messageValue(payload.preservedCommitCount, 0) };
        default:
            return {};
    }
}

function eventSummary(
    type: string,
    payload: RunEventPayload,
    allEvents: readonly TauriTavernAgentRunEvent[],
): string {
    switch (type) {
        case 'model_completed':
            return '';
        case 'agent_handoff_accepted':
            return joinValues(payload.sourceInvocationId, payload.workspaceKey);
        case 'agent_delegate_started':
        case 'agent_task_started':
        case 'agent_task_completed':
        case 'agent_task_failed':
        case 'agent_task_cancelled':
            return joinValues(payload.status, payload.workspaceKey);
        case 'agent_invocation_started':
        case 'agent_invocation_completed':
        case 'agent_invocation_failed':
        case 'agent_invocation_cancelled':
            return joinValues(payload.status, payload.kind);
        case 'task_return_completed':
            return joinValues(payload.status, firstString(payload.summaryRef, payload.resultRef));
        case 'context_assembled':
            return Array.isArray(payload.toolDiagnostics)
                ? payload.toolDiagnostics
                    .map(diagnostic => plainObject(diagnostic) ? stringValue(diagnostic.message) : '')
                    .filter(Boolean)
                    .join(' | ')
                : '';
        case 'tool_call_requested':
            return stringValue(payload.callId);
        case 'tool_call_completed':
            return textMetricsSummary(payload.displayMetrics)
                || textMetricsSummary(payload)
                || resourceSummary(payload.resourceRefs)
                || elapsedSummary(payload.elapsedMs);
        case 'tool_call_failed':
            return firstString(payload.message, payload.errorCode);
        case 'workspace_file_written':
        case 'direct_output_captured':
        case 'workspace_patch_applied':
            return fileSummary(payload);
        case 'chat_commit_requested':
            return commitSummary(payload);
        case 'chat_commit_completed':
            return commitCompletedSummary(payload, allEvents);
        case 'chat_commit_failed':
            return stringValue(payload.message);
        case 'persistent_changes_committed':
            return Array.isArray(payload.changes)
                ? payload.changes
                    .map(change => plainObject(change) ? stringValue(change.path) : '')
                    .filter(Boolean)
                    .join(', ')
                : '';
        case 'drift_recovery_attempted':
            return stringValue(payload.reasonCode);
        case 'user_guidance_submitted':
        case 'user_guidance_applied':
            return guidanceSummary(payload);
        case 'user_guidance_discarded':
            return joinValues(payload.reason, guidanceSummary(payload));
        case 'run_cancelled':
            return stringValue(payload.message);
        case 'run_partial_success':
            return partialSuccessSummary(payload);
        case 'run_failed':
            return failureSummary(payload);
        default:
            return '';
    }
}

function eventKind(type: string, payload: RunEventPayload, fallback?: string): string {
    if (type === 'agent_handoff_accepted') {
        return 'handoff';
    }
    if (type.startsWith('user_guidance_')) {
        return 'guidance';
    }
    if (type === 'tool_call_requested' || type === 'tool_call_completed') {
        return toolKind(payload.toolId);
    }
    return fallback || 'event';
}

function toolKind(toolId: unknown): string {
    const normalized = stringValue(toolId);
    if (normalized.startsWith('builtin:agent.') || normalized === 'builtin:task.return') return 'subagent';
    if (!normalized.startsWith('builtin:')) return 'tool';
    if (normalized.includes('read')) return 'read';
    if (normalized.includes('search')) return 'search';
    if (normalized.includes('list')) return 'list';
    if (normalized === 'builtin:workspace.write_file') return 'write';
    if (normalized === 'builtin:workspace.apply_patch') return 'patch';
    if (normalized === 'builtin:workspace.commit') return 'commit';
    if (normalized === 'builtin:workspace.finish') return 'done';
    return 'tool';
}

function fileSummary(payload: RunEventPayload): string {
    const parts: string[] = [];
    const metrics = textMetricsSummary(payload);
    if (metrics) parts.push(metrics);
    const replacements = scalarString(payload.replacements);
    if (replacements) parts.push(`${replacements} replacements`);
    return parts.join(' | ');
}

function commitSummary(payload: RunEventPayload): string {
    return joinValues(payload.mode, payload.reason, textMetricsSummary(payload));
}

function commitCompletedSummary(
    payload: RunEventPayload,
    events: readonly TauriTavernAgentRunEvent[],
): string {
    const messageId = stringValue(payload.messageId);
    const requested = findCommitRequestedEvent(events, payload.commitId);
    return joinValues(
        messageId ? `message ${messageId}` : payload.mode,
        textMetricsSummary(payload) || textMetricsSummary(requested?.payload),
    );
}

function guidanceSummary(payload: RunEventPayload): string {
    return stringValue(payload.preview).trim()
        || textMetricsSummary(payload)
        || normalizeGuidanceIds(payload).join(', ');
}

function normalizeGuidanceIds(payload: RunEventPayload): string[] {
    const ids = normalizeStringArray(payload.guidanceIds);
    const guidanceId = stringValue(payload.guidanceId).trim();
    return guidanceId ? [guidanceId, ...ids] : ids;
}

function normalizeStringArray(value: unknown): string[] {
    return Array.isArray(value)
        ? value.map(stringValue).map(item => item.trim()).filter(Boolean)
        : [];
}

function findCommitRequestedEvent(
    events: readonly TauriTavernAgentRunEvent[],
    commitId: unknown,
): TauriTavernAgentRunEvent | null {
    const normalized = stringValue(commitId).trim();
    if (!normalized) return null;
    return events.find((event) => {
        const payload = plainObject(event.payload) ? event.payload : {};
        return event.type === 'chat_commit_requested' && stringValue(payload.commitId) === normalized;
    }) ?? null;
}

function resourceSummary(resourceRefs: unknown): string {
    return Array.isArray(resourceRefs) ? resourceRefs.map(stringValue).filter(Boolean).join(', ') : '';
}

function elapsedSummary(value: unknown): string {
    const elapsed = Number(value);
    return Number.isFinite(elapsed) && elapsed > 0 ? `${Math.round(elapsed)} ms` : '';
}

function partialSuccessSummary(payload: RunEventPayload): string {
    const count = Number(payload.preservedCommitCount);
    if (Number.isInteger(count) && count > 0) {
        return `${count} committed message${count === 1 ? '' : 's'} preserved`;
    }
    return firstString(payload.message, payload.code);
}

function failureSummary(payload: RunEventPayload): string {
    return presentAgentRunFailure({ payload }).summary;
}

function messageValue(value: unknown, fallback: string | number): string | number | boolean {
    return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean'
        ? value
        : fallback;
}

function scalarString(value: unknown): string {
    return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean'
        ? String(value)
        : '';
}

function firstString(...values: unknown[]): string {
    return values.map(stringValue).find(Boolean) ?? '';
}

function joinValues(...values: unknown[]): string {
    return values.map(scalarString).filter(Boolean).join(' | ');
}

function stringValue(value: unknown): string {
    return typeof value === 'string' ? value : '';
}

function plainObject(value: unknown): value is RunEventPayload {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
