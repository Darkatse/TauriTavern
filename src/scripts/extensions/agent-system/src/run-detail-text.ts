import { translateAgentSystem as tr } from './i18n';
import type { AgentSystemMessageKey } from './i18n';
import { totalTextMetricsSummary } from './run-text-metrics';
import type {
    TimelineDetailBlock,
    TimelineDetailField,
    TimelineDetailTextBlock,
    TimelineDiffRow,
} from './RunTimelineContract';

export const DETAIL_TEXT_LIMIT = 40000;
export const NESTED_TEXT_LIMIT = 12000;

export type LineDiff = {
    rows: TimelineDiffRow[];
    addedLines: number;
    deletedLines: number;
};

type BlockLabel = AgentSystemMessageKey | { literal: string };
type TextBlockOptions = {
    kind?: 'text' | 'assistant' | 'reasoning' | 'user';
    meta?: string;
    defaultOpen?: boolean;
};

export function addBlock(
    blocks: TimelineDetailBlock[],
    label: BlockLabel,
    value: unknown,
    limit = DETAIL_TEXT_LIMIT,
    alreadyTruncated = false,
    options: TextBlockOptions = {},
): void {
    const text = typeof value === 'string' ? value : describeNestedValue(value);
    if (!text.trim()) return;
    blocks.push(textBlock(label, text, limit, alreadyTruncated, options));
}

export function textBlock(
    label: BlockLabel,
    value: string,
    limit = DETAIL_TEXT_LIMIT,
    alreadyTruncated = false,
    options: TextBlockOptions = {},
): TimelineDetailTextBlock {
    const truncated = truncateText(value, limit);
    const block = {
        text: truncated.text,
        truncated: alreadyTruncated || truncated.truncated,
        ...options,
    };
    return typeof label === 'string'
        ? { ...block, labelKey: label }
        : { ...block, label: label.literal };
}

export function field(label: string, value: unknown): TimelineDetailField {
    return { label, value: scalarText(value) };
}

export function requiredString(value: unknown, key: string): string {
    if (!plainObject(value) || typeof value[key] !== 'string') {
        throw new Error(tr('timelinePatchDiffMissingField', { field: key }));
    }
    return value[key];
}

export function buildLineDiff(oldText: string, newText: string): LineDiff {
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

    const rows: TimelineDiffRow[] = [];
    for (let index = 0; index < prefix; index += 1) {
        rows.push(diffRow('context', index + 1, index + 1, ' ', oldLines[index] ?? ''));
    }
    const oldChangedEnd = oldLines.length - suffix;
    const newChangedEnd = newLines.length - suffix;
    for (let index = prefix; index < oldChangedEnd; index += 1) {
        rows.push(diffRow('delete', index + 1, null, '-', oldLines[index] ?? ''));
    }
    for (let index = prefix; index < newChangedEnd; index += 1) {
        rows.push(diffRow('add', null, index + 1, '+', newLines[index] ?? ''));
    }
    for (let index = oldChangedEnd; index < oldLines.length; index += 1) {
        const newIndex = newChangedEnd + index - oldChangedEnd;
        rows.push(diffRow('context', index + 1, newIndex + 1, ' ', oldLines[index] ?? ''));
    }

    return {
        rows,
        addedLines: newChangedEnd - prefix,
        deletedLines: oldChangedEnd - prefix,
    };
}

export function reasoningMeta(item: unknown): string {
    const value = plainObject(item) ? item : {};
    const parts: string[] = [];
    if (typeof value.source === 'string' && value.source.trim()) parts.push(value.source.trim());
    const metrics = totalTextMetricsSummary(value);
    if (metrics) parts.push(metrics);
    return parts.join(' · ');
}

export function describeNestedValue(value: unknown): string {
    if (Array.isArray(value)) {
        return value.map((entry, index) => `${index + 1}. ${describeInlineValue(entry)}`).join('\n');
    }
    if (plainObject(value)) {
        return Object.entries(value)
            .map(([key, entry]) => `${labelForKey(key)}: ${describeInlineValue(entry)}`)
            .join('\n');
    }
    return isPrimitive(value) ? formatPrimitive(value) : '';
}

export function labelForKey(key: string): string {
    return key
        .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
        .replace(/[_-]+/g, ' ')
        .replace(/\b\w/g, character => character.toUpperCase());
}

export function joinStringArray(value: unknown): string {
    if (!Array.isArray(value)) return '';
    return value.map(scalarText).map(item => item.trim()).filter(Boolean).join(', ');
}

export function parseJson(text: string): { ok: true; value: unknown } | { ok: false; value: null } {
    try {
        return { ok: true, value: JSON.parse(text) as unknown };
    } catch {
        return { ok: false, value: null };
    }
}

export function plainObject(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function truncateText(text: string, limit: number): { text: string; truncated: boolean } {
    if (text.length <= limit) return { text, truncated: false };
    return { text: `${text.slice(0, limit)}\n...`, truncated: true };
}

function splitDiffLines(text: string): string[] {
    if (!text) return [];
    const lines = text.split('\n');
    if (text.endsWith('\n')) lines.pop();
    return lines;
}

function diffRow(
    type: TimelineDiffRow['type'],
    oldLine: number | null,
    newLine: number | null,
    marker: TimelineDiffRow['marker'],
    text: string,
): TimelineDiffRow {
    return { type, oldLine, newLine, marker, text };
}

function describeInlineValue(value: unknown): string {
    if (isPrimitive(value)) return formatPrimitive(value);
    if (Array.isArray(value)) return value.map(describeInlineValue).join(', ');
    if (plainObject(value)) {
        return Object.entries(value)
            .map(([key, entry]) => `${labelForKey(key)}=${describeInlineValue(entry)}`)
            .join(', ');
    }
    return '';
}

function formatPrimitive(value: string | number | boolean): string {
    return typeof value === 'boolean' ? (value ? 'yes' : 'no') : String(value);
}

function scalarText(value: unknown): string {
    return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean'
        ? String(value)
        : '';
}

function isPrimitive(value: unknown): value is string | number | boolean {
    return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean';
}
