import { leadNear, prettyJson } from './format';
import type { LeadMark, OutlineRow } from './outline';
import { type DiffLine, changedRanges } from './text-diff';

export const UNCHANGED_LABEL = '{count} unchanged';

/** Both requests number their rows the same way; the prefix keeps the ids of the base's rows apart. */
const BASE_ID = 'base:';

/**
 * Whitespace at the end of a text is what joins a piece to the next one: it
 * changes with the neighbour, not with the piece.
 */
export function content(row: OutlineRow): string {
    switch (row.body?.kind) {
        case 'text': return row.body.text.trimEnd();
        case 'json': return prettyJson(row.body.value);
        case 'tool': return [row.body.description, prettyJson(row.body.schema)].filter(Boolean).join('\n\n');
        default: return '';
    }
}

function lineCount(lines: DiffLine[], op: DiffLine['op']): number {
    return lines.reduce((sum, line) => line.op === op ? sum + 1 : sum, 0);
}

function changedLead(lines: DiffLine[]): { lead: string; mark: LeadMark | null } {
    for (let start = 0; start < lines.length; start++) {
        if (lines[start]?.op === 'same') {
            continue;
        }
        let end = start;
        while (end < lines.length && lines[end]?.op !== 'same') {
            end += 1;
        }
        const stretch = lines.slice(start, end);
        const removed = stretch.filter(line => line.op === 'removed');
        const added = stretch.filter(line => line.op === 'added');
        const index = added.findIndex(line => line.text.trim() !== '');
        const now = added[index];
        if (now) {
            const before = removed[index];
            const range = before ? changedRanges(before.text, now.text)?.after[0] : undefined;
            const { lead, offset } = leadNear(now.text, range?.start ?? 0);
            const mark = range
                ? { start: Math.min(offset, lead.length), end: Math.min(offset + range.end - range.start, lead.length) }
                : { start: 0, end: lead.length };
            return { lead, mark: { op: 'added', ...mark } };
        }
        const gone = removed.find(line => line.text.trim() !== '');
        if (gone) {
            const { lead } = leadNear(gone.text, 0);
            return { lead, mark: { op: 'removed', start: 0, end: lead.length } };
        }
        start = end;
    }
    return { lead: '', mark: null };
}

export function changedRow(after: OutlineRow, lines: DiffLine[]): OutlineRow {
    const added = lineCount(lines, 'added');
    const removed = lineCount(lines, 'removed');
    // The rows differ in their lead alone, as calls of two tools with the same arguments do.
    if (added + removed === 0) {
        return { ...after, change: { kind: 'changed', added, removed, mark: null } };
    }
    const { lead, mark } = changedLead(lines);
    return { ...after, lead, body: { kind: 'diff', lines }, change: { kind: 'changed', added, removed, mark } };
}

/** A path points into the body of the request, which a row of the base is not in. */
export function removedRow(row: OutlineRow): OutlineRow {
    return { ...row, id: `${BASE_ID}${row.id}`, path: null, change: { kind: 'removed' } };
}

export function addedRow(row: OutlineRow): OutlineRow {
    return { ...row, change: { kind: 'added' } };
}
