import { displayToolName } from './run-tool-labels.js';

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

    const addFile = (labelKey, path) => {
        const normalized = String(path || '').trim();
        if (!normalized || seenPaths.has(normalized)) {
            return;
        }
        seenPaths.add(normalized);
        targets.push({ type: 'file', labelKey, path: normalized });
    };

    addFile('timelineArguments', payload.argumentsRef);
    addFile('timelineModelResponse', payload.modelResponsePath);

    if (event?.type === 'tool_call_completed' || event?.type === 'tool_call_failed') {
        const resultPath = findToolResultPath(allEvents, payload.callId);
        addFile('timelineToolResult', resultPath);
    }

    if (event?.type === 'workspace_file_written'
        || event?.type === 'workspace_patch_applied'
        || event?.type === 'chat_commit_requested'
        || event?.type === 'chat_commit_completed') {
        addFile('timelineWorkspaceFile', payload.path);
    }

    return targets;
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
        case 'run_cancelled':
        case 'run_failed':
            return payload.message || '';
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
