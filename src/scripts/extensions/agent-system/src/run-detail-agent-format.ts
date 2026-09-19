import { translateAgentSystem as tr, type AgentSystemMessageKey } from './i18n';
import { addBlock, describeNestedValue, field, labelForKey, NESTED_TEXT_LIMIT } from './run-detail-text';
import { subAgentStatusLabel } from './run-timeline-display';
import type { TimelineDetailAction, TimelineDetailBlock, TimelineDetailSection, TimelineDetailTarget } from './RunTimelineContract';

type TaskTarget = Extract<TimelineDetailTarget, { type: 'agentTask' }>;

const FIELD_LABELS: Readonly<Record<string, AgentSystemMessageKey>> = {
    title: 'timelineDetailFieldTask',
    objective: 'timelineTaskObjective',
    context: 'timelineTaskContext',
    expectedOutput: 'timelineTaskExpectedOutput',
    reason: 'timelineDetailFieldReason',
    contextSummary: 'timelineTaskContext',
    workspaceRefs: 'timelineTaskReferences',
    mustPreserve: 'timelineTaskPreserve',
    completionCriteria: 'timelineTaskCompletionCriteria',
    findings: 'timelineTaskFindings',
    suggestedNextActions: 'timelineTaskNextActions',
    artifacts: 'timelineTaskArtifacts',
    confidence: 'timelineTaskConfidence',
    taskId: 'timelineDetailFieldTask',
    parentInvocationId: 'timelineDetailFieldSourceInvocation',
    childInvocationId: 'timelineDetailFieldInvocation',
    workspaceKey: 'timelineDetailFieldWorkspace',
    resultRef: 'timelineSubAgentResult',
    summaryRef: 'timelineSubAgentSummary',
};

export function formatAgentTaskDetail(
    target: TaskTarget,
    task: TauriTavernAgentTaskDetail,
): TimelineDetailSection {
    const brief = task.task;
    const fields = [
        field(tr('timelineDetailFieldAgent'), task.targetProfileId),
        field(tr('timelineDetailFieldStatus'), subAgentStatusLabel(task.status, tr)),
    ];
    if (brief.title?.trim()) {
        fields.push(field(tr('timelineDetailFieldTask'), brief.title));
    }
    const blocks: TimelineDetailBlock[] = [];
    const actions: TimelineDetailAction[] = [];
    const result = task.result;
    if (target.view === 'result') {
        if (result) {
            addBlock(blocks, 'timelineSubAgentSummary', result.summary);
            addBlock(blocks, 'timelineTaskWarnings', result.output.warnings);
            addBlock(blocks, 'timelineTaskQuestions', result.output.questionsForCaller);
            addCollapsedBlock(blocks, 'timelineTaskMoreResult', packetDetails(result.output,
                ['summary', 'status', 'warnings', 'questionsForCaller']));
        } else if (!task.error) {
            addBlock(blocks, 'timelineSubAgentResult', tr('timelineTaskResultUnavailable'));
        }
        addCollapsedBlock(blocks, 'timelineTaskBrief', packetDetails(brief, ['title']));
    } else {
        addBlock(blocks, 'timelineTaskObjective', brief.objective);
        addCollapsedBlock(blocks, task.continuation === 'transfer_control' ? 'timelineHandoffBrief' : 'timelineTaskBrief',
            packetDetails(brief, ['title', 'objective']));
    }
    if (task.error) addBlock(blocks, 'timelineErrorDetails', task.error);
    addCollapsedBlock(blocks, 'timelineTechnicalDetails', packetDetails({
        taskId: task.taskId,
        parentInvocationId: task.parentInvocationId,
        childInvocationId: task.childInvocationId,
        workspaceKey: task.workspaceKey,
        resultRef: task.resultRef,
        summaryRef: result?.summaryRef,
    }));
    if (task.continuation === 'return_to_parent') {
        actions.push({
            kind: 'openSubAgent',
            labelKey: 'timelineActionOpenSubAgent',
            hintKey: 'timelineActionOpenSubAgentHint',
            icon: 'fa-up-right-from-square',
            invocationId: task.childInvocationId,
        });
    }
    return { labelKey: target.labelKey, path: '', fields, blocks, actions };
}

function packetDetails(packet: Record<string, unknown>, omitted: readonly string[] = []): string {
    return Object.entries(packet).flatMap(([key, value]) => {
        if (omitted.includes(key)) return [];
        const text = describeNestedValue(value).trim();
        if (!text) return [];
        const labelKey = FIELD_LABELS[key];
        return [`${labelKey ? tr(labelKey) : labelForKey(key)}\n${text}`];
    }).join('\n\n');
}

function addCollapsedBlock(blocks: TimelineDetailBlock[], label: AgentSystemMessageKey, text: string): void {
    addBlock(blocks, label, text, NESTED_TEXT_LIMIT, false, { defaultOpen: false });
}
