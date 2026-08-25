import { presentAgentRunFailure } from '../../../tauritavern/agent/agent-error-presenter.js';
import { translateAgentSystem as tr } from './i18n';
import {
    formatArgumentsSection,
    formatToolResultSection,
    renderModelToolCalls,
} from './run-detail-tool-format';
import {
    addBlock,
    buildLineDiff,
    DETAIL_TEXT_LIMIT,
    field,
    joinStringArray,
    NESTED_TEXT_LIMIT,
    parseJson,
    plainObject,
    reasoningMeta,
    requiredString,
    textBlock,
} from './run-detail-text';
import { textMetricsSummary } from './run-text-metrics';
import type {
    TimelineDetailAction,
    TimelineDetailBlock,
    TimelineDetailField,
    TimelineDetailSection,
    TimelineDetailTarget,
} from './RunTimelineContract';

type FileDetailTarget = Extract<TimelineDetailTarget, { type: 'file' }>;
type ModelDetailTarget = Extract<TimelineDetailTarget, { type: 'modelTurn' | 'modelReasoning' | 'modelNarration' }>;
type SubAgentDetailTarget = Extract<TimelineDetailTarget, { type: 'subAgentTask' }>;
type HandoffDetailTarget = Extract<TimelineDetailTarget, { type: 'handoff' }>;
type GuidanceDetailTarget = Extract<TimelineDetailTarget, { type: 'guidance' }>;
type PatchDiffDetailTarget = Extract<TimelineDetailTarget, { type: 'patchDiff' }>;
type RunFailureDetailTarget = Extract<TimelineDetailTarget, { type: 'runFailure' }>;
type WorkspaceFile = Awaited<ReturnType<TauriTavernAgentApi['readWorkspaceFile']>>;
export function formatDetailFile(
    target: FileDetailTarget,
    file: WorkspaceFile,
): TimelineDetailSection {
    const parsed = parseJson(file.text);
    if (target.labelKey === 'timelineArguments' && parsed.ok && plainObject(parsed.value)) {
        return formatArgumentsSection(target, file, parsed.value);
    }
    if (target.labelKey === 'timelineToolResult' && parsed.ok && plainObject(parsed.value)) {
        return formatToolResultSection(target, file, parsed.value);
    }
    return formatTextFileSection(target, file);
}

export function formatModelTurnDetail(
    target: ModelDetailTarget,
    turn: TauriTavernAgentModelTurn,
): TimelineDetailSection {
    const fields: TimelineDetailField[] = [
        field(tr('timelineDetailFieldRound'), turn.round ?? target.round),
    ];
    if (target.invocationId) fields.push(field(tr('timelineDetailFieldInvocation'), target.invocationId));
    if (turn.provider.source || turn.provider.format) {
        fields.push(field(
            tr('timelineDetailFieldProvider'),
            [turn.provider.source, turn.provider.format].filter(Boolean).join(' / '),
        ));
    }
    if (turn.provider.model) fields.push(field(tr('timelineDetailFieldModel'), turn.provider.model));

    const blocks: TimelineDetailBlock[] = [];
    if (target.type === 'modelNarration' && turn.narration?.text.trim()) {
        addBlock(blocks, 'timelineNarration', turn.narration.text, DETAIL_TEXT_LIMIT, turn.narration.truncated, {
            kind: 'assistant',
            meta: textMetricsSummary({
                chars: turn.narration.totalChars,
                words: turn.narration.totalWords,
            }),
        });
    }
    if (target.type === 'modelTurn' && turn.assistant.text.trim()) {
        addBlock(blocks, 'timelineAssistantText', turn.assistant.text, DETAIL_TEXT_LIMIT, turn.assistant.truncated, {
            kind: 'assistant',
            meta: textMetricsSummary({
                chars: turn.assistant.totalChars,
                words: turn.assistant.totalWords,
            }),
        });
    }
    if (target.type !== 'modelNarration') {
        for (const item of turn.reasoning) {
            addBlock(blocks, 'timelineReasoning', item.text, DETAIL_TEXT_LIMIT, item.truncated, {
                kind: 'reasoning',
                defaultOpen: false,
                meta: reasoningMeta(item),
            });
        }
    }
    if (target.type === 'modelTurn' && turn.toolCalls.length > 0) {
        addBlock(blocks, 'timelineModelToolCalls', renderModelToolCalls(turn.toolCalls), NESTED_TEXT_LIMIT, false, {
            defaultOpen: false,
        });
    }

    return {
        labelKey: target.labelKey,
        path: target.showPath ? turn.modelResponsePath : '',
        fields,
        blocks,
    };
}

export function formatSubAgentTaskDetail(target: SubAgentDetailTarget): TimelineDetailSection {
    const fields: TimelineDetailField[] = [];
    const actions: TimelineDetailAction[] = [];
    if (target.targetProfileId) fields.push(field(tr('timelineDetailFieldAgent'), target.targetProfileId));
    if (target.status) fields.push(field(tr('timelineDetailFieldStatus'), target.status));
    if (target.workspaceKey) fields.push(field(tr('timelineDetailFieldWorkspace'), target.workspaceKey));
    if (target.taskId) fields.push(field(tr('timelineDetailFieldTask'), target.taskId));
    if (target.childInvocationId) {
        fields.push(field(tr('timelineDetailFieldInvocation'), target.childInvocationId));
        actions.push({
            kind: 'openSubAgent',
            labelKey: 'timelineActionOpenSubAgent',
            hintKey: 'timelineActionOpenSubAgentHint',
            icon: 'fa-up-right-from-square',
            invocationId: target.childInvocationId,
        });
    }
    if (target.error) fields.push(field(tr('timelineDetailFieldErrorCode'), target.error));

    const blocks: TimelineDetailBlock[] = [];
    if (target.summaryRef) addBlock(blocks, 'timelineSubAgentSummary', target.summaryRef);
    if (target.resultRef) addBlock(blocks, 'timelineSubAgentResult', target.resultRef);
    return { labelKey: target.labelKey, path: '', fields, blocks, actions };
}

export function formatHandoffDetail(target: HandoffDetailTarget): TimelineDetailSection {
    const fields: TimelineDetailField[] = [];
    if (target.targetProfileId) fields.push(field(tr('timelineDetailFieldAgent'), target.targetProfileId));
    if (target.status) fields.push(field(tr('timelineDetailFieldStatus'), target.status));
    if (target.workspaceKey) fields.push(field(tr('timelineDetailFieldWorkspace'), target.workspaceKey));
    if (target.sourceInvocationId) fields.push(field(tr('timelineDetailFieldSourceInvocation'), target.sourceInvocationId));
    if (target.newInvocationId) fields.push(field(tr('timelineDetailFieldInvocation'), target.newInvocationId));
    if (target.taskId) fields.push(field(tr('timelineDetailFieldTask'), target.taskId));
    return { labelKey: target.labelKey, path: '', fields, blocks: [], actions: [] };
}

export function formatGuidanceDetail(target: GuidanceDetailTarget): TimelineDetailSection {
    const fields: TimelineDetailField[] = [];
    const blocks: TimelineDetailBlock[] = [];
    const guidanceIds = joinStringArray(target.guidanceIds);
    if (guidanceIds) fields.push(field(tr('timelineDetailFieldGuidance'), guidanceIds));
    const clientGuidanceIds = joinStringArray(target.clientGuidanceIds);
    if (clientGuidanceIds) fields.push(field(tr('timelineDetailFieldClient'), clientGuidanceIds));
    if (target.status) fields.push(field(tr('timelineDetailFieldStatus'), target.status));
    if (target.invocationId) fields.push(field(tr('timelineDetailFieldInvocation'), target.invocationId));
    if (target.round != null) fields.push(field(tr('timelineDetailFieldRound'), target.round));
    if (target.reason) fields.push(field(tr('timelineDetailFieldReason'), target.reason));
    const metrics = textMetricsSummary(target);
    if (metrics) fields.push(field(tr('timelineDetailFieldTextMetrics'), metrics));
    const text = (target.text || target.preview).trim();
    if (text) addBlock(blocks, 'timelineContent', text, DETAIL_TEXT_LIMIT, false, { kind: 'user' });
    return { labelKey: target.labelKey, path: '', fields, blocks, actions: [] };
}

export function formatPatchDiffDetail(
    target: PatchDiffDetailTarget,
    file: WorkspaceFile,
): TimelineDetailSection {
    const parsed = parseJson(file.text);
    if (!parsed.ok || !plainObject(parsed.value)) {
        throw new Error(tr('timelinePatchDiffInvalidArguments'));
    }
    const path = requiredString(parsed.value, 'path');
    if (path !== target.path) {
        throw new Error(tr('timelinePatchDiffPathMismatch', { expected: target.path, actual: path }));
    }
    const oldString = requiredString(parsed.value, 'old_string');
    const newString = requiredString(parsed.value, 'new_string');
    if (!oldString) throw new Error(tr('timelinePatchDiffEmptyOldString'));

    const diff = buildLineDiff(oldString, newString);
    const fields: TimelineDetailField[] = [field(tr('timelineDetailFieldTarget'), path)];
    if (target.replacements != null) {
        fields.push(field(tr('timelineDetailFieldReplacements'), target.replacements));
    }
    const metrics = textMetricsSummary(target);
    if (metrics) fields.push(field(tr('timelineDetailFieldTextMetrics'), metrics));
    if (parsed.value.replace_all === true) {
        fields.push(field(tr('timelineDetailFieldReplaceAll'), tr('timelineDetailStatusYes')));
    }
    return {
        labelKey: target.labelKey,
        path,
        fields,
        blocks: [{
            kind: 'diff',
            labelKey: 'timelinePatchDiff',
            rows: diff.rows,
            meta: `+${diff.addedLines} / -${diff.deletedLines}`,
            defaultOpen: true,
        }],
    };
}

export function formatRunFailureDetail(
    target: RunFailureDetailTarget,
    options: { allowRetry?: boolean } = {},
): TimelineDetailSection {
    if (target.event.type === 'run_partial_success') return formatRunPartialSuccessDetail(target);
    const presentation = presentAgentRunFailure(target.event);
    const fields: TimelineDetailField[] = [];
    const blocks: TimelineDetailBlock[] = [];
    const actions: TimelineDetailAction[] = [];
    if (presentation.code) fields.push(field(tr('timelineDetailFieldErrorCode'), presentation.code));
    fields.push(field(tr('timelineDetailFieldRetryable'), presentation.retryable));
    fields.push(field(tr('timelineDetailFieldUserRetryable'), presentation.userRetryable));
    addBlock(blocks, 'timelineResultText', presentation.message);
    if (presentation.technicalMessage && presentation.technicalMessage !== presentation.message) {
        addBlock(blocks, 'timelineTechnicalMessage', presentation.technicalMessage, DETAIL_TEXT_LIMIT, false, {
            defaultOpen: false,
        });
    }
    if (presentation.userRetryable && options.allowRetry !== false) {
        actions.push({
            kind: 'retry',
            labelKey: 'timelineActionRetry',
            hintKey: 'timelineActionRetryHint',
            icon: 'fa-rotate-right',
        });
    }
    return { labelKey: target.labelKey, path: '', fields, blocks, actions };
}

function formatRunPartialSuccessDetail(target: RunFailureDetailTarget): TimelineDetailSection {
    const payload = plainObject(target.event.payload) ? target.event.payload : {};
    const fields: TimelineDetailField[] = [];
    const blocks: TimelineDetailBlock[] = [];
    const code = stringValue(payload.code).trim();
    const message = stringValue(payload.message).trim();
    const technicalMessage = (stringValue(payload.technicalMessage) || message).trim();
    const preservedCommitCount = Number(payload.preservedCommitCount);
    if (code) fields.push(field(tr('timelineDetailFieldErrorCode'), code));
    if (Number.isInteger(preservedCommitCount)) {
        fields.push(field(tr('timelineDetailFieldPreservedCommits'), preservedCommitCount));
    }
    fields.push(field(tr('timelineDetailFieldRetryable'), false));
    fields.push(field(tr('timelineDetailFieldUserRetryable'), false));
    addBlock(blocks, 'timelinePartialSuccessMessage', tr('timelinePartialSuccessDetail'));
    if (message) addBlock(blocks, 'timelineResultText', message);
    if (technicalMessage && technicalMessage !== message) {
        addBlock(blocks, 'timelineTechnicalMessage', technicalMessage, DETAIL_TEXT_LIMIT, false, {
            defaultOpen: false,
        });
    }
    return { labelKey: target.labelKey, path: '', fields, blocks, actions: [] };
}

function formatTextFileSection(target: FileDetailTarget, file: WorkspaceFile): TimelineDetailSection {
    const metrics = textMetricsSummary(target) || textMetricsSummary(file);
    const fields = metrics ? [field(tr('timelineDetailFieldTextMetrics'), metrics)] : [];
    return {
        labelKey: target.labelKey,
        path: file.path || target.path,
        fields,
        blocks: [textBlock('timelineContent', file.text)],
    };
}

function stringValue(value: unknown): string {
    return typeof value === 'string' ? value : '';
}
