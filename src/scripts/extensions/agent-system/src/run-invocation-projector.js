export const ROOT_INVOCATION_ID = 'inv_root';
export const RETURN_TO_PARENT_CONTINUATION = 'return_to_parent';
export const TRANSFER_CONTROL_CONTINUATION = 'transfer_control';

const SUBAGENT_COLORS = Object.freeze([
    '#5fa6a0',
    '#7c9bd6',
    '#c59a50',
    '#bf7493',
    '#7daf63',
    '#b084cc',
]);

const TASK_TERMINAL_STATUSES = new Set(['completed', 'failed', 'cancelled']);

export function projectAgentInvocations(timelineProjection) {
    const invocations = new Map();
    for (const invocation of Array.isArray(timelineProjection?.invocations) ? timelineProjection.invocations : []) {
        invocations.set(normalizeInvocationId(invocation.invocationId), invocation);
    }

    const taskList = (Array.isArray(timelineProjection?.delegationEdges) ? timelineProjection.delegationEdges : [])
        .map((edge) => ({
            ...edge,
            childInvocationId: edge.targetInvocationId,
        }));
    const tasks = new Map(taskList.map((task) => [task.taskId, task]));

    const subAgentTasks = taskList
        .filter((task) => task.continuation === RETURN_TO_PARENT_CONTINUATION)
        .filter((task) => task.childInvocationId)
        .sort(compareByCreatedAt)
        .map((task, index) => ({
            ...task,
            color: SUBAGENT_COLORS[index % SUBAGENT_COLORS.length],
            displayName: task.targetProfileId || task.workspaceKey || task.childInvocationId,
            invocation: invocations.get(task.childInvocationId) || null,
        }));
    const handoffTasks = taskList
        .filter((task) => task.continuation === TRANSFER_CONTROL_CONTINUATION)
        .filter((task) => task.childInvocationId)
        .sort(compareByCreatedAt)
        .map((task) => ({
            ...task,
            displayName: task.targetProfileId || task.workspaceKey || task.childInvocationId,
            invocation: invocations.get(task.childInvocationId) || null,
        }));

    return {
        invocations,
        tasks,
        subAgentTasks,
        handoffTasks,
        foregroundInvocationIds: normalizeForegroundInvocationIds(timelineProjection?.foregroundInvocationIds),
        runningSubAgentCount: subAgentTasks.filter((task) => isActiveTaskStatus(task.status)).length,
        terminalSubAgentCount: subAgentTasks.filter((task) => TASK_TERMINAL_STATUSES.has(task.status)).length,
        failedSubAgentCount: subAgentTasks.filter((task) => task.status === 'failed').length,
    };
}

export function eventBelongsToInvocation(event, invocationId) {
    const normalized = normalizeInvocationId(invocationId);
    const payload = plainObject(event?.payload) ? event.payload : {};
    const type = String(event?.type || '');
    const scoped = eventBelongsToCanonicalScope(payload, normalized);
    if (scoped !== null) {
        return scoped;
    }

    if (normalized === ROOT_INVOCATION_ID) {
        if (type.startsWith('run_')) {
            return true;
        }
        if (type === 'agent_delegate_started') {
            return normalizeInvocationId(payload.parentInvocationId) === ROOT_INVOCATION_ID;
        }
        if (type.startsWith('agent_task_')) {
            return false;
        }
        if (payload.childInvocationId && normalizeInvocationId(payload.childInvocationId) !== ROOT_INVOCATION_ID) {
            return false;
        }
        if (payload.newInvocationId && normalizeInvocationId(payload.newInvocationId) !== ROOT_INVOCATION_ID) {
            return false;
        }
        return normalizeInvocationId(payload.invocationId) === ROOT_INVOCATION_ID;
    }

    return normalizeInvocationId(payload.invocationId) === normalized
        || normalizeInvocationId(payload.parentInvocationId) === normalized
        || normalizeInvocationId(payload.sourceInvocationId) === normalized
        || normalizeInvocationId(payload.childInvocationId) === normalized
        || normalizeInvocationId(payload.newInvocationId) === normalized;
}

export function normalizeInvocationId(value) {
    return String(value || '').trim() || ROOT_INVOCATION_ID;
}

export function isRootInvocation(value) {
    return normalizeInvocationId(value) === ROOT_INVOCATION_ID;
}

export function isActiveTaskStatus(status) {
    return status === 'queued' || status === 'running';
}

function normalizeForegroundInvocationIds(values) {
    const ids = [];
    for (const value of Array.isArray(values) ? values : [ROOT_INVOCATION_ID]) {
        const id = normalizeInvocationId(value);
        if (!ids.includes(id)) {
            ids.push(id);
        }
    }
    return ids;
}

function compareByCreatedAt(left, right) {
    return String(left.createdAt || '').localeCompare(String(right.createdAt || ''))
        || String(left.taskId || '').localeCompare(String(right.taskId || ''));
}

function eventBelongsToCanonicalScope(payload, invocationId) {
    const scope = plainObject(payload?.eventScope) ? payload.eventScope : null;
    if (!scope) {
        return null;
    }
    const scopeInvocationId = String(scope.invocationId || '').trim();
    const relatedInvocationIds = Array.isArray(scope.relatedInvocationIds)
        ? scope.relatedInvocationIds.map((value) => String(value || '').trim()).filter(Boolean)
        : null;
    if (!scopeInvocationId && relatedInvocationIds == null) {
        return null;
    }
    return (scopeInvocationId ? normalizeInvocationId(scopeInvocationId) === invocationId : false)
        || (relatedInvocationIds || []).some((value) => normalizeInvocationId(value) === invocationId);
}

function plainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
