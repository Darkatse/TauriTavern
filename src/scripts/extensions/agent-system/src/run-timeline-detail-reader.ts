import { requireHostApi } from './host-api';
import { translateAgentSystem as tr } from './i18n';
import {
    formatDetailFile,
    formatGuidanceDetail,
    formatModelTurnDetail,
    formatPatchDiffDetail,
    formatRunFailureDetail,
} from './run-detail-format';
import { formatAgentTaskDetail } from './run-detail-agent-format';
import { isRootInvocation } from './run-invocation-projector';
import type {
    TimelineDetailReadInput,
    TimelineDetailSection,
    TimelineDetailTarget,
} from './RunTimelineContract';

export async function readTimelineDetailSections({
    runId,
    targets,
    readOnly = false,
}: TimelineDetailReadInput): Promise<TimelineDetailSection[]> {
    const normalizedRunId = requireRunId(runId);
    const sections: TimelineDetailSection[] = [];
    for (const target of targets) {
        sections.push(await readTimelineDetailTarget({
            runId: normalizedRunId,
            target,
            readOnly,
        }));
    }
    return sections;
}

async function readTimelineDetailTarget(input: {
    runId: string;
    target: TimelineDetailTarget;
    readOnly: boolean;
}): Promise<TimelineDetailSection> {
    const { runId, target, readOnly } = input;
    if (target.type === 'agentTask') {
        const api = requireHostApi('agent');
        const task = await api.readTaskDetail({
            runId,
            taskId: target.taskId,
            includeResult: target.view === 'result',
        });
        return formatAgentTaskDetail(target, task);
    }
    if (target.type === 'guidance') {
        return formatGuidanceDetail(target);
    }
    if (target.type === 'modelTurn' || target.type === 'modelReasoning' || target.type === 'modelNarration') {
        const modelInput: Parameters<TauriTavernAgentApi['readModelTurn']>[0] = {
            runId,
            round: target.round,
        };
        if (target.invocationId && !isRootInvocation(target.invocationId)) {
            modelInput.invocationId = target.invocationId;
        }
        const turn = await requireHostApi('agent').readModelTurn(modelInput);
        return formatModelTurnDetail(target, turn);
    }
    if (target.type === 'patchDiff') {
        if (target.errorKey) {
            throw new Error(tr(target.errorKey, target.errorParams));
        }
        const file = await requireHostApi('agent').readWorkspaceFile({
            runId,
            path: target.argumentsRef,
        });
        return formatPatchDiffDetail(target, file);
    }
    if (target.type === 'runFailure') {
        return formatRunFailureDetail(target, {
            allowRetry: !readOnly,
        });
    }

    if (target.type === 'file') {
        const file = await requireHostApi('agent').readWorkspaceFile({
            runId,
            path: target.path,
        });
        return formatDetailFile(target, file);
    }
    throw new Error('Unsupported Agent timeline detail target');
}

function requireRunId(value: string): string {
    const runId = value.trim();
    if (!runId) {
        throw new Error('Agent run id is required.');
    }
    return runId;
}
