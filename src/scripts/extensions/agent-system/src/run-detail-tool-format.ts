import { translateAgentSystem as tr } from './i18n';
import {
    addBlock,
    describeNestedValue,
    field,
    labelForKey,
    NESTED_TEXT_LIMIT,
    plainObject,
} from './run-detail-text';
import { displayToolName } from './run-tool-labels';
import { textMetricsSummary } from './run-text-metrics';
import type {
    TimelineDetailBlock,
    TimelineDetailField,
    TimelineDetailSection,
    TimelineDetailTarget,
} from './RunTimelineContract';

type FileDetailTarget = Extract<TimelineDetailTarget, { type: 'file' }>;
type WorkspaceFile = Awaited<ReturnType<TauriTavernAgentApi['readWorkspaceFile']>>;
type RunEventPayload = Record<string, unknown>;

const ARGUMENT_BLOCK_KEYS: ReadonlySet<string> = new Set([
    'content',
    'old_string',
    'new_string',
    'text',
    'prompt',
    'message',
]);

export function formatArgumentsSection(
    target: FileDetailTarget,
    file: WorkspaceFile,
    args: RunEventPayload,
): TimelineDetailSection {
    const fields: TimelineDetailField[] = [];
    const blocks: TimelineDetailBlock[] = [];

    for (const [key, value] of Object.entries(args)) {
        if (value == null) continue;
        if (ARGUMENT_BLOCK_KEYS.has(key)) {
            addBlock(blocks, { literal: labelForKey(key) }, value);
        } else if (isPrimitive(value)) {
            fields.push(field(labelForKey(key), primitiveText(value)));
        } else {
            addBlock(blocks, { literal: labelForKey(key) }, describeNestedValue(value), NESTED_TEXT_LIMIT);
        }
    }

    return {
        labelKey: target.labelKey,
        path: file.path || target.path,
        fields,
        blocks,
    };
}

export function formatToolResultSection(
    target: FileDetailTarget,
    file: WorkspaceFile,
    result: RunEventPayload,
): TimelineDetailSection {
    const name = nativeToolName(result.toolId);
    const structured = plainObject(result.structured) ? result.structured : {};
    const fields: TimelineDetailField[] = [
        field(tr('timelineDetailFieldOperation'), displayToolName(name)),
    ];
    const blocks: TimelineDetailBlock[] = [];

    addToolResultSummaryFields(fields, result, structured);
    const hits = Array.isArray(structured.hits) ? structured.hits : [];
    if (hits.length > 0) {
        addBlock(blocks, 'timelineMatches', renderHits(hits), NESTED_TEXT_LIMIT);
    } else if (typeof result.content === 'string' && result.content.trim()) {
        addBlock(blocks, 'timelineResultText', toolContentForDisplay(result.content, name));
    }

    return {
        labelKey: target.labelKey,
        path: file.path || target.path,
        fields,
        blocks,
    };
}

export function renderModelToolCalls(toolCalls: TauriTavernAgentModelTurn['toolCalls']): string {
    return toolCalls.map((call, index) => {
        const name = displayToolName(call.name);
        const identity = call.toolId.startsWith('builtin:') ? '' : ` [${call.toolId}]`;
        const id = call.callId ? ` ${call.callId}` : '';
        return `${index + 1}. ${name}${identity}${id}`;
    }).join('\n');
}

function addToolResultSummaryFields(
    fields: TimelineDetailField[],
    result: RunEventPayload,
    structured: RunEventPayload,
): void {
    if (result.isError === true) {
        fields.push(field(tr('timelineDetailFieldStatus'), tr('timelineDetailStatusError')));
    }
    if (typeof result.errorCode === 'string' && result.errorCode) {
        fields.push(field(tr('timelineDetailFieldErrorCode'), result.errorCode));
    }
    if (typeof structured.query === 'string' && structured.query.trim()) {
        fields.push(field(tr('timelineDetailFieldQuery'), structured.query.trim()));
    }

    const target = primaryTarget(result, structured);
    if (target) fields.push(field(tr('timelineDetailFieldTarget'), target));
    const range = rangeSummary(structured);
    if (range) fields.push(field(tr('timelineDetailFieldRange'), range));
    if (Array.isArray(structured.hits)) {
        fields.push(field(tr('timelineDetailFieldMatches'), structured.hits.length));
    }
    const metrics = textMetricsSummary(structured);
    if (metrics) fields.push(field(tr('timelineDetailFieldTextMetrics'), metrics));
}

function primaryTarget(result: RunEventPayload, structured: RunEventPayload): string {
    if (typeof structured.resourceRef === 'string' && structured.resourceRef.trim()) {
        return structured.resourceRef.trim();
    }
    if (Array.isArray(result.resourceRefs) && result.resourceRefs.length === 1) {
        return stringValue(result.resourceRefs[0]).trim();
    }
    return typeof structured.path === 'string' ? structured.path.trim() : '';
}

function rangeSummary(structured: RunEventPayload): string {
    const startLine = Number(structured.startLine);
    const endLine = Number(structured.endLine);
    const totalLines = Number(structured.totalLines);
    if (!Number.isFinite(startLine) || !Number.isFinite(endLine) || startLine <= 0 || endLine <= 0) {
        return '';
    }
    if (structured.fullRead === true
        || (startLine === 1 && Number.isFinite(totalLines) && endLine === totalLines)) {
        return tr('timelineDetailRangeFull');
    }
    return tr('timelineDetailRangeLines', { start: startLine, end: endLine });
}

function toolContentForDisplay(content: string, name: string): string {
    const normalized = content.trim();
    if ((name === 'workspace.read_file' || name === 'skill.read') && normalized.includes('\n')) {
        return normalized.slice(normalized.indexOf('\n') + 1).trim();
    }
    return normalized;
}

function nativeToolName(toolId: unknown): string {
    if (typeof toolId !== 'string') throw new TypeError('tool result requires a canonical toolId');
    const separator = toolId.indexOf(':');
    if (separator <= 0 || separator === toolId.length - 1) {
        throw new TypeError('tool result requires a canonical toolId');
    }
    return toolId.slice(separator + 1);
}

function renderHits(hits: unknown[]): string {
    return hits.map((hitValue, index) => {
        const hit = plainObject(hitValue) ? hitValue : {};
        const path = firstString(hit.path, hit.ref, hit.refId) || 'result';
        const startLine = positiveNumber(hit.startLine);
        const endLine = positiveNumber(hit.endLine);
        const range = startLine && endLine ? ` L${startLine}-L${endLine}` : '';
        const scoreValue = Number(hit.score);
        const score = Number.isFinite(scoreValue) ? ` score ${scoreValue.toFixed(2)}` : '';
        const metrics = textMetricsSummary(hit);
        const metric = metrics ? ` ${metrics}` : '';
        const snippet = typeof hit.snippet === 'string' && hit.snippet.trim()
            ? `\n${indentLines(hit.snippet.trim())}`
            : '';
        return `${index + 1}. ${path}${range}${score}${metric}${snippet}`;
    }).join('\n\n');
}

function indentLines(text: string): string {
    return text.split('\n').map(line => `  ${line}`).join('\n');
}

function primitiveText(value: string | number | boolean): string {
    return typeof value === 'boolean' ? (value ? 'yes' : 'no') : String(value);
}

function positiveNumber(value: unknown): number | null {
    const number = Number(value);
    return Number.isFinite(number) && number > 0 ? number : null;
}

function firstString(...values: unknown[]): string {
    return values.map(stringValue).find(Boolean) ?? '';
}

function stringValue(value: unknown): string {
    return typeof value === 'string' ? value : '';
}

function isPrimitive(value: unknown): value is string | number | boolean {
    return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean';
}
