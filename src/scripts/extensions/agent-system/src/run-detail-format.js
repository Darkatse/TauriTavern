import { displayToolName } from './run-tool-labels.js';
import { translateAgentSystem as tr } from './i18n.js';
import { presentAgentRunFailure } from '../../../tauritavern/agent/agent-error-presenter.js';

const DETAIL_TEXT_LIMIT = 40000;
const NESTED_TEXT_LIMIT = 12000;

const ARGUMENT_BLOCK_KEYS = new Set([
    'content',
    'old_string',
    'new_string',
    'text',
    'prompt',
    'message',
]);

export function formatDetailFile(target, file) {
    const text = String(file?.text || '');
    const parsed = parseJson(text);

    if (target.labelKey === 'timelineArguments' && parsed.ok && plainObject(parsed.value)) {
        return formatArgumentsSection(target, file, parsed.value);
    }
    if (target.labelKey === 'timelineToolResult' && parsed.ok && plainObject(parsed.value)) {
        return formatToolResultSection(target, file, parsed.value);
    }

    return formatTextFileSection(target, file, text);
}

export function formatModelTurnDetail(target, turn) {
    const fields = [
        field(tr('timelineDetailFieldRound'), turn?.round ?? target.round),
    ];
    const provider = turn?.provider || {};
    if (provider.source || provider.format) {
        fields.push(field(tr('timelineDetailFieldProvider'), [provider.source, provider.format].filter(Boolean).join(' / ')));
    }
    if (provider.model) {
        fields.push(field(tr('timelineDetailFieldModel'), provider.model));
    }

    const blocks = [];
    for (const item of Array.isArray(turn?.reasoning) ? turn.reasoning : []) {
        addBlock(blocks, 'timelineReasoning', item.text, DETAIL_TEXT_LIMIT, item.truncated === true, {
            kind: 'reasoning',
            defaultOpen: false,
            meta: reasoningMeta(item),
        });
    }

    return {
        labelKey: target.labelKey,
        path: target.showPath ? turn?.modelResponsePath || '' : '',
        fields,
        blocks,
    };
}

export function formatPatchDiffDetail(target, file) {
    const parsed = parseJson(String(file?.text || ''));
    if (!parsed.ok || !plainObject(parsed.value)) {
        throw new Error(tr('timelinePatchDiffInvalidArguments'));
    }

    const args = parsed.value;
    const path = requiredString(args, 'path');
    if (path !== target.path) {
        throw new Error(tr('timelinePatchDiffPathMismatch', { expected: target.path, actual: path }));
    }

    const oldString = requiredString(args, 'old_string');
    const newString = requiredString(args, 'new_string');
    if (!oldString) {
        throw new Error(tr('timelinePatchDiffEmptyOldString'));
    }

    const diff = buildLineDiff(oldString, newString);
    const fields = [
        field(tr('timelineDetailFieldTarget'), path),
    ];
    if (target.replacements != null) {
        fields.push(field(tr('timelineDetailFieldReplacements'), target.replacements));
    }
    if (args.replace_all === true) {
        fields.push(field(tr('timelineDetailFieldReplaceAll'), tr('timelineDetailStatusYes')));
    }

    return {
        labelKey: target.labelKey,
        path,
        fields,
        blocks: [
            {
                kind: 'diff',
                labelKey: 'timelinePatchDiff',
                rows: diff.rows,
                meta: `+${diff.addedLines} / -${diff.deletedLines}`,
                defaultOpen: true,
            },
        ],
    };
}

export function formatRunFailureDetail(target) {
    if (target?.event?.type === 'run_partial_success') {
        return formatRunPartialSuccessDetail(target);
    }

    const presentation = presentAgentRunFailure(target.event);
    const fields = [];
    const blocks = [];
    const actions = [];

    if (presentation.code) {
        fields.push(field(tr('timelineDetailFieldErrorCode'), presentation.code));
    }
    fields.push(field(tr('timelineDetailFieldRetryable'), String(presentation.retryable)));
    fields.push(field(tr('timelineDetailFieldUserRetryable'), String(presentation.userRetryable)));
    addBlock(blocks, 'timelineResultText', presentation.message);
    if (presentation.technicalMessage && presentation.technicalMessage !== presentation.message) {
        addBlock(blocks, 'timelineTechnicalMessage', presentation.technicalMessage, DETAIL_TEXT_LIMIT, false, {
            defaultOpen: false,
        });
    }

    if (presentation.userRetryable) {
        actions.push({
            kind: 'retry',
            labelKey: 'timelineActionRetry',
            hintKey: 'timelineActionRetryHint',
            icon: 'fa-rotate-right',
        });
    }

    return {
        labelKey: target.labelKey,
        path: '',
        fields,
        blocks,
        actions,
    };
}

function formatRunPartialSuccessDetail(target) {
    const payload = target?.event?.payload || {};
    const fields = [];
    const blocks = [];
    const code = String(payload.code || '').trim();
    const message = String(payload.message || '').trim();
    const technicalMessage = String(payload.technicalMessage || message).trim();
    const preservedCommitCount = Number(payload.preservedCommitCount);

    if (code) {
        fields.push(field(tr('timelineDetailFieldErrorCode'), code));
    }
    if (Number.isInteger(preservedCommitCount)) {
        fields.push(field(tr('timelineDetailFieldPreservedCommits'), String(preservedCommitCount)));
    }
    fields.push(field(tr('timelineDetailFieldRetryable'), 'false'));
    fields.push(field(tr('timelineDetailFieldUserRetryable'), 'false'));

    addBlock(blocks, 'timelinePartialSuccessMessage', tr('timelinePartialSuccessDetail'));
    if (message) {
        addBlock(blocks, 'timelineResultText', message);
    }
    if (technicalMessage && technicalMessage !== message) {
        addBlock(blocks, 'timelineTechnicalMessage', technicalMessage, DETAIL_TEXT_LIMIT, false, {
            defaultOpen: false,
        });
    }

    return {
        labelKey: target.labelKey,
        path: '',
        fields,
        blocks,
        actions: [],
    };
}

function formatArgumentsSection(target, file, args) {
    const fields = [];
    const blocks = [];

    for (const [key, value] of Object.entries(args)) {
        if (value == null) {
            continue;
        }
        if (ARGUMENT_BLOCK_KEYS.has(key)) {
            addBlock(blocks, labelForKey(key), value);
            continue;
        }
        if (isPrimitive(value)) {
            fields.push(field(labelForKey(key), formatPrimitive(value)));
            continue;
        }
        addBlock(blocks, labelForKey(key), describeNestedValue(value), NESTED_TEXT_LIMIT);
    }

    return {
        labelKey: target.labelKey,
        path: file?.path || target.path,
        fields,
        blocks,
    };
}

function formatToolResultSection(target, file, result) {
    const structured = plainObject(result.structured) ? result.structured : {};
    const fields = [field(tr('timelineDetailFieldOperation'), displayToolName(result.name))];
    const blocks = [];

    addToolResultSummaryFields(fields, result, structured);

    const hits = Array.isArray(structured.hits) ? structured.hits : [];
    if (hits.length > 0) {
        addBlock(blocks, 'timelineMatches', renderHits(hits), NESTED_TEXT_LIMIT);
    } else if (typeof result.content === 'string' && result.content.trim()) {
        addBlock(blocks, 'timelineResultText', toolContentForDisplay(result));
    }

    return {
        labelKey: target.labelKey,
        path: file?.path || target.path,
        fields,
        blocks,
    };
}

function addToolResultSummaryFields(fields, result, structured) {
    if (result.isError) {
        fields.push(field(tr('timelineDetailFieldStatus'), tr('timelineDetailStatusError')));
    }
    if (result.errorCode) {
        fields.push(field(tr('timelineDetailFieldErrorCode'), result.errorCode));
    }

    if (typeof structured.query === 'string' && structured.query.trim()) {
        fields.push(field(tr('timelineDetailFieldQuery'), structured.query.trim()));
    }

    const target = primaryTarget(result, structured);
    if (target) {
        fields.push(field(tr('timelineDetailFieldTarget'), target));
    }

    const range = rangeSummary(structured);
    if (range) {
        fields.push(field(tr('timelineDetailFieldRange'), range));
    }

    if (Array.isArray(structured.hits)) {
        fields.push(field(tr('timelineDetailFieldMatches'), String(structured.hits.length)));
    }
}

function primaryTarget(result, structured) {
    if (typeof structured.resourceRef === 'string' && structured.resourceRef.trim()) {
        return structured.resourceRef.trim();
    }
    if (Array.isArray(result.resourceRefs) && result.resourceRefs.length === 1) {
        return String(result.resourceRefs[0] || '').trim();
    }
    if (typeof structured.path === 'string' && structured.path.trim()) {
        return structured.path.trim();
    }
    return '';
}

function rangeSummary(structured) {
    const startLine = Number(structured.startLine);
    const endLine = Number(structured.endLine);
    const totalLines = Number(structured.totalLines);
    if (Number.isFinite(startLine) && Number.isFinite(endLine) && startLine > 0 && endLine > 0) {
        if (structured.fullRead === true || (startLine === 1 && Number.isFinite(totalLines) && endLine === totalLines)) {
            return tr('timelineDetailRangeFull');
        }
        return tr('timelineDetailRangeLines', { start: startLine, end: endLine });
    }

    const startChar = Number(structured.startChar);
    const endChar = Number(structured.endChar);
    const totalChars = Number(structured.totalChars);
    if (Number.isFinite(startChar) && Number.isFinite(endChar) && endChar > startChar) {
        if (structured.fullRead === true || (startChar === 0 && Number.isFinite(totalChars) && endChar === totalChars)) {
            return tr('timelineDetailRangeFull');
        }
        return tr('timelineDetailRangeChars', { start: startChar, end: endChar });
    }
    return '';
}

function toolContentForDisplay(result) {
    const content = String(result.content || '').trim();
    if ((result.name === 'workspace.read_file' || result.name === 'skill.read') && content.includes('\n')) {
        return content.slice(content.indexOf('\n') + 1).trim();
    }
    return content;
}

function formatTextFileSection(target, file, text) {
    return {
        labelKey: target.labelKey,
        path: file?.path || target.path,
        fields: [],
        blocks: [
            textBlock('timelineContent', text),
        ],
    };
}

function renderHits(hits) {
    return hits.map((hit, index) => {
        const path = hit.path || hit.ref || hit.refId || 'result';
        const range = hit.startLine && hit.endLine ? ` L${hit.startLine}-L${hit.endLine}` : '';
        const score = Number.isFinite(Number(hit.score)) ? ` score ${Number(hit.score).toFixed(2)}` : '';
        const snippet = typeof hit.snippet === 'string' && hit.snippet.trim()
            ? `\n${indentLines(hit.snippet.trim())}`
            : '';
        return `${index + 1}. ${path}${range}${score}${snippet}`;
    }).join('\n\n');
}

function addBlock(blocks, label, value, limit = DETAIL_TEXT_LIMIT, alreadyTruncated = false, options = {}) {
    const text = typeof value === 'string' ? value : describeNestedValue(value);
    if (!text.trim()) {
        return;
    }
    blocks.push(textBlock(label, text, limit, alreadyTruncated, options));
}

function textBlock(label, value, limit = DETAIL_TEXT_LIMIT, alreadyTruncated = false, options = {}) {
    const truncated = truncateText(String(value || ''), limit);
    const block = {
        text: truncated.text,
        truncated: alreadyTruncated || truncated.truncated,
        ...options,
    };
    if (label.startsWith('timeline')) {
        block.labelKey = label;
    } else {
        block.label = label;
    }
    return block;
}

function requiredString(value, key) {
    if (!plainObject(value) || typeof value[key] !== 'string') {
        throw new Error(tr('timelinePatchDiffMissingField', { field: key }));
    }
    return value[key];
}

function buildLineDiff(oldText, newText) {
    const oldLines = splitDiffLines(oldText);
    const newLines = splitDiffLines(newText);
    let prefix = 0;
    while (prefix < oldLines.length
        && prefix < newLines.length
        && oldLines[prefix] === newLines[prefix]) {
        prefix += 1;
    }

    let suffix = 0;
    while (suffix < oldLines.length - prefix
        && suffix < newLines.length - prefix
        && oldLines[oldLines.length - 1 - suffix] === newLines[newLines.length - 1 - suffix]) {
        suffix += 1;
    }

    const rows = [];
    for (let index = 0; index < prefix; index += 1) {
        rows.push(diffRow('context', index + 1, index + 1, ' ', oldLines[index]));
    }

    const oldChangedEnd = oldLines.length - suffix;
    const newChangedEnd = newLines.length - suffix;
    for (let index = prefix; index < oldChangedEnd; index += 1) {
        rows.push(diffRow('delete', index + 1, null, '-', oldLines[index]));
    }
    for (let index = prefix; index < newChangedEnd; index += 1) {
        rows.push(diffRow('add', null, index + 1, '+', newLines[index]));
    }

    for (let index = oldChangedEnd; index < oldLines.length; index += 1) {
        const newIndex = newChangedEnd + index - oldChangedEnd;
        rows.push(diffRow('context', index + 1, newIndex + 1, ' ', oldLines[index]));
    }

    return {
        rows,
        addedLines: newChangedEnd - prefix,
        deletedLines: oldChangedEnd - prefix,
    };
}

function diffRow(type, oldLine, newLine, marker, text) {
    return {
        type,
        oldLine,
        newLine,
        marker,
        text,
    };
}

function splitDiffLines(text) {
    const value = String(text);
    if (!value) {
        return [];
    }

    const lines = value.split('\n');
    if (value.endsWith('\n')) {
        lines.pop();
    }
    return lines;
}

function reasoningMeta(item) {
    const parts = [];
    if (typeof item?.source === 'string' && item.source.trim()) {
        parts.push(item.source.trim());
    }
    if (Number.isFinite(Number(item?.bytes)) && Number(item.bytes) > 0) {
        parts.push(tr('timelineBytes', { count: Number(item.bytes) }));
    }
    return parts.join(' · ');
}

function truncateText(text, limit) {
    if (text.length <= limit) {
        return { text, truncated: false };
    }
    return {
        text: `${text.slice(0, limit)}\n...`,
        truncated: true,
    };
}

function describeNestedValue(value) {
    if (Array.isArray(value)) {
        return value.map((entry, index) => `${index + 1}. ${describeInlineValue(entry)}`).join('\n');
    }
    if (plainObject(value)) {
        return Object.entries(value)
            .map(([key, entry]) => `${labelForKey(key)}: ${describeInlineValue(entry)}`)
            .join('\n');
    }
    return formatPrimitive(value);
}

function describeInlineValue(value) {
    if (isPrimitive(value)) {
        return formatPrimitive(value);
    }
    if (Array.isArray(value)) {
        return value.map(describeInlineValue).join(', ');
    }
    if (plainObject(value)) {
        return Object.entries(value)
            .map(([key, entry]) => `${labelForKey(key)}=${describeInlineValue(entry)}`)
            .join(', ');
    }
    return '';
}

function field(label, value) {
    return { label, value: String(value) };
}

function formatPrimitive(value) {
    if (typeof value === 'boolean') {
        return value ? 'yes' : 'no';
    }
    return String(value);
}

function labelForKey(key) {
    return String(key)
        .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
        .replace(/[_-]+/g, ' ')
        .replace(/\b\w/g, (character) => character.toUpperCase());
}

function indentLines(text) {
    return String(text)
        .split('\n')
        .map((line) => `  ${line}`)
        .join('\n');
}

function parseJson(text) {
    try {
        return { ok: true, value: JSON.parse(text) };
    } catch {
        return { ok: false, value: null };
    }
}

function isPrimitive(value) {
    return ['string', 'number', 'boolean'].includes(typeof value);
}

function plainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
