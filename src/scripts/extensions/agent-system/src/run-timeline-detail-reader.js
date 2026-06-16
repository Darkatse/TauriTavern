import { requireHostApi } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import {
    formatDetailFile,
    formatHandoffDetail,
    formatModelTurnDetail,
    formatPatchDiffDetail,
    formatRunFailureDetail,
    formatSubAgentTaskDetail,
} from './run-detail-format.js';
import { isRootInvocation } from './run-invocation-projector.js';

export async function readTimelineDetailSections({ runId, targets, readOnly = false }) {
    const normalizedRunId = requireRunId(runId);
    if (!Array.isArray(targets)) {
        throw new Error('Agent timeline detail targets must be an array.');
    }

    const sections = [];
    for (const target of targets) {
        sections.push(await readTimelineDetailTarget({
            runId: normalizedRunId,
            target,
            readOnly,
        }));
    }
    return sections;
}

export async function readTimelineDetailTarget({ runId, target, readOnly = false }) {
    const normalizedRunId = requireRunId(runId);
    if (target.type === 'handoff') {
        return formatHandoffDetail(target);
    }
    if (target.type === 'subAgentTask') {
        return formatSubAgentTaskDetail(target);
    }
    if (target.type === 'modelTurn' || target.type === 'modelReasoning' || target.type === 'modelNarration') {
        const input = {
            runId: normalizedRunId,
            round: target.round,
        };
        if (target.invocationId && !isRootInvocation(target.invocationId)) {
            input.invocationId = target.invocationId;
        }
        const turn = await requireHostApi('agent').readModelTurn(input);
        return formatModelTurnDetail(target, turn);
    }
    if (target.type === 'patchDiff') {
        if (target.errorKey) {
            throw new Error(tr(target.errorKey, target.errorParams || {}));
        }
        const file = await requireHostApi('agent').readWorkspaceFile({
            runId: normalizedRunId,
            path: target.argumentsRef,
        });
        return formatPatchDiffDetail(target, file);
    }
    if (target.type === 'runFailure') {
        return formatRunFailureDetail(target, {
            allowRetry: !readOnly,
        });
    }

    const file = await requireHostApi('agent').readWorkspaceFile({
        runId: normalizedRunId,
        path: target.path,
    });
    return formatDetailFile(target, file);
}

function requireRunId(value) {
    const runId = String(value || '').trim();
    if (!runId) {
        throw new Error('Agent run id is required.');
    }
    return runId;
}
