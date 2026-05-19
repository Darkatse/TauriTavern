import { displayToolName } from './run-tool-labels.js';
import { presentAgentRunFailure } from '../../../tauritavern/agent/agent-error-presenter.js';

const DISPLAY_EVENT_TYPES = new Set([
    'tool_call_requested',
    'tool_call_completed',
    'tool_call_failed',
    'workspace_file_written',
    'workspace_patch_applied',
    'chat_commit_requested',
    'chat_commit_completed',
    'chat_commit_failed',
    'persistent_changes_committed',
    'drift_recovery_attempted',
    'run_completed',
    'run_cancelled',
    'run_failed',
]);

export const TERMINAL_EVENT_TYPES = Object.freeze(['run_completed', 'run_cancelled', 'run_failed']);

const SIDE_EFFECT_TOOL_COMPLETIONS = new Set([
    'workspace.write_file',
    'workspace.apply_patch',
    'workspace.commit',
    'workspace.finish',
]);

const SIDE_EFFECT_TOOL_BY_EVENT_TYPE = Object.freeze({
    workspace_file_written: 'workspace.write_file',
    workspace_patch_applied: 'workspace.apply_patch',
    chat_commit_requested: 'workspace.commit',
    chat_commit_completed: 'workspace.commit',
    chat_commit_failed: 'workspace.commit',
    persistent_changes_committed: 'workspace.finish',
    run_completed: 'workspace.finish',
});

const EVENT_META = Object.freeze({
    tool_call_requested: { icon: 'fa-screwdriver-wrench', tone: 'active', kind: 'tool', titleKey: 'timelineEventToolRequested' },
    tool_call_completed: { icon: 'fa-check', tone: 'success', kind: 'tool', titleKey: 'timelineEventToolCompleted' },
    tool_call_failed: { icon: 'fa-triangle-exclamation', tone: 'warn', kind: 'fail', titleKey: 'timelineEventToolFailed' },
    workspace_file_written: { icon: 'fa-file-lines', tone: 'success', kind: 'write', titleKey: 'timelineEventFileWritten' },
    workspace_patch_applied: { icon: 'fa-code-commit', tone: 'success', kind: 'patch', titleKey: 'timelineEventPatchApplied' },
    chat_commit_requested: { icon: 'fa-message', tone: 'active', kind: 'commit', titleKey: 'timelineEventCommitRequested' },
    chat_commit_completed: { icon: 'fa-circle-check', tone: 'success', kind: 'commit', titleKey: 'timelineEventCommitCompleted' },
    chat_commit_failed: { icon: 'fa-circle-exclamation', tone: 'error', kind: 'fail', titleKey: 'timelineEventCommitFailed' },
    persistent_changes_committed: { icon: 'fa-database', tone: 'success', kind: 'persist', titleKey: 'timelineEventPersistentCommitted' },
    drift_recovery_attempted: { icon: 'fa-arrows-rotate', tone: 'warn', kind: 'recover', titleKey: 'timelineEventDriftRecoveryAttempted' },
    run_completed: { icon: 'fa-circle-check', tone: 'success', kind: 'done', titleKey: 'timelineEventRunCompleted' },
    run_cancelled: { icon: 'fa-ban', tone: 'warn', kind: 'cancel', titleKey: 'timelineEventRunCancelled' },
    run_failed: { icon: 'fa-circle-xmark', tone: 'error', kind: 'fail', titleKey: 'timelineEventRunFailed' },
});

export function isDisplayableRunEvent(event) {
    return DISPLAY_EVENT_TYPES.has(String(event?.type || ''));
}

export function timelineItemsFromEvents(events) {
    const completedToolCalls = new Set();
    const resolvedCommits = new Set();

    for (const event of events) {
        if (event?.type === 'tool_call_completed' || event?.type === 'tool_call_failed') {
            const callId = String(event?.payload?.callId || '').trim();
            if (callId) {
                completedToolCalls.add(callId);
            }
        }
        if (event?.type === 'chat_commit_completed' || event?.type === 'chat_commit_failed') {
            const commitId = String(event?.payload?.commitId || '').trim();
            if (commitId) {
                resolvedCommits.add(commitId);
            }
        }
    }

    return events
        .filter((event) => shouldShowEvent(event, completedToolCalls, resolvedCommits))
        .map(presentRunEvent);
}

export function presentRunEvent(event) {
    const type = String(event?.type || '');
    const payload = plainObject(event?.payload) ? event.payload : {};
    const meta = EVENT_META[type] || {
        icon: 'fa-circle',
        tone: event?.level === 'error' ? 'error' : 'info',
        titleKey: 'timelineEventGeneric',
    };

    return {
        id: String(event?.id || `${event?.runId || 'run'}:${event?.seq || type}`),
        seq: Number(event?.seq || 0),
        runId: String(event?.runId || ''),
        type,
        level: String(event?.level || 'info'),
        timestamp: String(event?.timestamp || ''),
        icon: meta.icon,
        tone: event?.level === 'error' ? 'error' : meta.tone,
        kind: eventKind(type, payload, meta.kind),
        titleKey: meta.titleKey,
        titleParams: eventTitleParams(type, payload),
        summary: eventSummary(type, payload),
        rawEvent: event,
    };
}

export function buildEventDetailTargets(item, allEvents) {
    const event = item?.rawEvent;
    const payload = plainObject(event?.payload) ? event.payload : {};
    const targets = [];
    const seenPaths = new Set();
    const seenReasoningRounds = new Set();

    const addFile = (labelKey, path) => {
        const normalized = String(path || '').trim();
        if (!normalized || seenPaths.has(normalized)) {
            return;
        }
        seenPaths.add(normalized);
        targets.push({ type: 'file', labelKey, path: normalized });
    };
    const addModelReasoning = (round) => {
        const normalized = Number(round);
        if (!Number.isInteger(normalized) || normalized <= 0) {
            return;
        }
        if (!modelTurnHasReasoning(allEvents, normalized)) {
            return;
        }
        if (seenReasoningRounds.has(normalized)) {
            return;
        }
        seenReasoningRounds.add(normalized);
        targets.push({ type: 'modelReasoning', labelKey: 'timelineReasoning', round: normalized });
    };

    addModelReasoning(payload.round);
    addModelReasoning(findAssociatedToolRound(event, allEvents));
    addFile('timelineArguments', payload.argumentsRef);

    if (event?.type === 'tool_call_completed' || event?.type === 'tool_call_failed') {
        const resultPath = findToolResultPath(allEvents, payload.callId);
        addFile('timelineToolResult', resultPath);
    }

    if (event?.type === 'workspace_patch_applied') {
        targets.push(buildPatchDiffTarget(event, allEvents));
    }

    if (event?.type === 'run_failed') {
        targets.push({ type: 'runFailure', labelKey: 'timelineErrorDetails', event });
    }

    if (event?.type === 'workspace_file_written'
        || event?.type === 'workspace_patch_applied'
        || event?.type === 'chat_commit_requested'
        || event?.type === 'chat_commit_completed') {
        addFile('timelineWorkspaceFile', payload.path);
    }

    return targets;
}

function buildPatchDiffTarget(event, events) {
    const payload = plainObject(event?.payload) ? event.payload : {};
    const path = String(payload.path || '').trim();
    const completed = findSideEffectToolCompletion(events, event, 'workspace.apply_patch', path);
    const callId = String(completed?.payload?.callId || '').trim();
    const requested = callId ? findToolRequest(events, callId) : null;
    const requestPayload = plainObject(requested?.payload) ? requested.payload : {};
    const argumentsRef = String(requestPayload.argumentsRef || '').trim();

    return {
        type: 'patchDiff',
        labelKey: 'timelinePatchDiff',
        path,
        argumentsRef,
        replacements: payload.replacements,
        bytes: payload.bytes,
        errorKey: path && argumentsRef ? '' : 'timelinePatchDiffSourceMissing',
        errorParams: { path },
    };
}

function shouldShowEvent(event, completedToolCalls, resolvedCommits) {
    if (!isDisplayableRunEvent(event)) {
        return false;
    }

    const payload = plainObject(event?.payload) ? event.payload : {};
    if (event.type === 'tool_call_requested') {
        const callId = String(payload.callId || '').trim();
        return !callId || !completedToolCalls.has(callId);
    }
    if (event.type === 'tool_call_completed') {
        return !SIDE_EFFECT_TOOL_COMPLETIONS.has(String(payload.name || ''));
    }
    if (event.type === 'chat_commit_requested') {
        const commitId = String(payload.commitId || '').trim();
        return !commitId || !resolvedCommits.has(commitId);
    }
    return true;
}

function findToolResultPath(events, callId) {
    const normalized = String(callId || '').trim();
    if (!normalized) {
        return '';
    }

    const resultEvent = [...events]
        .reverse()
        .find((event) => event?.type === 'tool_result_stored'
            && String(event?.payload?.callId || '') === normalized);
    return resultEvent?.payload?.path || '';
}

function findAssociatedToolRound(event, events) {
    const payload = plainObject(event?.payload) ? event.payload : {};
    const callId = String(payload.callId || '').trim();
    if (callId) {
        return findToolEventRound(events, callId);
    }

    const toolName = SIDE_EFFECT_TOOL_BY_EVENT_TYPE[event?.type];
    if (!toolName) {
        return null;
    }

    const path = String(payload.path || '').trim();
    const completed = findSideEffectToolCompletion(events, event, toolName, path);
    return completed?.payload?.round ?? null;
}

function findToolEventRound(events, callId) {
    const event = events.find((candidate) => {
        if (candidate?.type !== 'tool_call_requested'
            && candidate?.type !== 'tool_call_completed'
            && candidate?.type !== 'tool_call_failed') {
            return false;
        }
        return String(candidate?.payload?.callId || '') === callId;
    });
    return event?.payload?.round ?? null;
}

function findSideEffectToolCompletion(events, sideEffectEvent, toolName, path) {
    const sideEffectSeq = Number(sideEffectEvent?.seq || 0);
    return [...events]
        .reverse()
        .find((event) => {
            if (event?.type !== 'tool_call_completed' || Number(event?.seq || 0) >= sideEffectSeq) {
                return false;
            }

            const payload = plainObject(event?.payload) ? event.payload : {};
            if (payload.name !== toolName) {
                return false;
            }

            return !path || (Array.isArray(payload.resourceRefs) && payload.resourceRefs.includes(path));
        });
}

function findToolRequest(events, callId) {
    return events.find((event) => event?.type === 'tool_call_requested'
        && String(event?.payload?.callId || '') === callId);
}

function modelTurnHasReasoning(events, round) {
    return events.some((event) => {
        if (event?.type !== 'model_completed') {
            return false;
        }
        const payload = plainObject(event?.payload) ? event.payload : {};
        return Number(payload.round) === round
            && (payload.hasReasoning === true || Number(payload.reasoningBytes) > 0);
    });
}

function eventTitleParams(type, payload) {
    switch (type) {
        case 'tool_call_requested':
        case 'tool_call_completed':
        case 'tool_call_failed':
            return { tool: toolLabel(payload.name) };
        case 'workspace_file_written':
        case 'workspace_patch_applied':
        case 'chat_commit_requested':
        case 'chat_commit_completed':
            return { path: payload.path || '' };
        case 'persistent_changes_committed':
            return { count: payload.changeCount ?? 0 };
        case 'drift_recovery_attempted':
            return { attempt: payload.attempt ?? 0, max: payload.maxAttempts ?? 0 };
        default:
            return {};
    }
}

function eventSummary(type, payload) {
    switch (type) {
        case 'tool_call_requested':
            return payload.callId || '';
        case 'tool_call_completed':
            return resourceSummary(payload.resourceRefs) || elapsedSummary(payload.elapsedMs);
        case 'tool_call_failed':
            return payload.message || payload.errorCode || '';
        case 'workspace_file_written':
        case 'workspace_patch_applied':
            return fileSummary(payload);
        case 'chat_commit_requested':
            return commitSummary(payload);
        case 'chat_commit_completed':
            return payload.messageId ? `message ${payload.messageId}` : payload.mode || '';
        case 'chat_commit_failed':
            return payload.message || '';
        case 'persistent_changes_committed':
            return Array.isArray(payload.changes) ? payload.changes.map((change) => change.path).filter(Boolean).join(', ') : '';
        case 'drift_recovery_attempted':
            return payload.reasonCode || '';
        case 'run_cancelled':
            return payload.message || '';
        case 'run_failed':
            return presentAgentRunFailure({ payload }).summary;
        default:
            return '';
    }
}

function eventKind(type, payload, fallback) {
    if (type === 'tool_call_requested' || type === 'tool_call_completed') {
        return toolKind(payload.name);
    }
    return fallback || 'event';
}

function toolKind(name) {
    const normalized = String(name || '');
    if (normalized.includes('read')) {
        return 'read';
    }
    if (normalized.includes('search')) {
        return 'search';
    }
    if (normalized.includes('list')) {
        return 'list';
    }
    if (normalized === 'workspace.write_file') {
        return 'write';
    }
    if (normalized === 'workspace.apply_patch') {
        return 'patch';
    }
    if (normalized === 'workspace.commit') {
        return 'commit';
    }
    if (normalized === 'workspace.finish') {
        return 'done';
    }
    return 'tool';
}

function fileSummary(payload) {
    const parts = [];
    if (payload.bytes != null) {
        parts.push(`${payload.bytes} bytes`);
    }
    if (payload.replacements != null) {
        parts.push(`${payload.replacements} replacements`);
    }
    return parts.join(' | ');
}

function commitSummary(payload) {
    const parts = [payload.mode, payload.reason, payload.bytes != null ? `${payload.bytes} bytes` : ''];
    return parts.filter(Boolean).join(' | ');
}

function resourceSummary(resourceRefs) {
    if (!Array.isArray(resourceRefs) || resourceRefs.length === 0) {
        return '';
    }
    return resourceRefs.join(', ');
}

function elapsedSummary(value) {
    const elapsed = Number(value);
    if (!Number.isFinite(elapsed) || elapsed <= 0) {
        return '';
    }
    return `${Math.round(elapsed)} ms`;
}

function toolLabel(name) {
    return displayToolName(name);
}

function plainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
