import type { Translate } from './RequestInspectorContract';
import { UNCHANGED_LABEL, addedRow, changedRow, content, removedRow } from './compare-row';
import { fill, firstLine, isScalar } from './format';
import type { OutlineRow, RowChange, TableEntry } from './outline';
import { emptyRow } from './outline-message';
import { diffLines } from './text-diff';

function sameValue(before: unknown, after: unknown): boolean {
    return JSON.stringify(before) === JSON.stringify(after);
}

function compareTable(before: TableEntry[], after: TableEntry[]): { entries: TableEntry[]; lead: string } | null {
    const was = new Map(before.map(entry => [entry.key, entry.value]));
    const now = new Set(after.map(entry => entry.key));
    const entries: TableEntry[] = [
        ...after.map(({ key, value }): TableEntry => {
            if (!was.has(key)) {
                return { key, value, change: { kind: 'added' } };
            }
            const old = was.get(key);
            return { key, value, change: sameValue(old, value) ? null : { kind: 'changed', before: old } };
        }),
        ...before.filter(entry => !now.has(entry.key)).map((entry): TableEntry => ({ ...entry, change: { kind: 'removed' } })),
    ];
    const told = entries.flatMap(({ key, value, change }) => {
        switch (change?.kind) {
            case 'added': return `+ ${key}${isScalar(value) ? ` ${String(value)}` : ''}`;
            case 'removed': return `− ${key}`;
            case 'changed': return isScalar(value) && isScalar(change.before)
                ? `${key} ${String(change.before)} → ${String(value)}`
                : `~ ${key}`;
            default: return [];
        }
    });
    return told.length > 0 ? { entries, lead: firstLine(told.join(' · ')) } : null;
}

function compareTools(before: OutlineRow[], after: OutlineRow[], tr: Translate): { rows: OutlineRow[]; lead: string } | null {
    const was = new Map(before.map(row => [row.name, row]));
    const now = new Set(after.map(row => row.name));
    const same: OutlineRow[] = [];
    const differing: OutlineRow[] = [];
    for (const row of after) {
        const old = was.get(row.name);
        if (!old) {
            differing.push(addedRow(row));
        } else if (content(old) === content(row)) {
            same.push(row);
        } else {
            differing.push(changedRow(row, diffLines(content(old), content(row))));
        }
    }
    differing.push(...before.filter(row => !now.has(row.name)).map(removedRow));
    if (differing.length === 0) {
        return null;
    }
    const signs = { added: '+', removed: '−', changed: '~' } as const;
    const rest: OutlineRow[] = same.length === 0 ? [] : [{
        ...emptyRow('tools.same', null),
        lead: fill(tr(UNCHANGED_LABEL), { count: same.length }),
        body: { kind: 'rows', rows: same },
    }];
    return {
        rows: [...differing, ...rest],
        lead: firstLine(differing.map(row => `${row.change ? signs[row.change.kind] : ''} ${row.name}`).join(' · ')),
    };
}

const CHANGED: RowChange = { kind: 'changed', added: 0, removed: 0, mark: null };

export function compareHead(before: OutlineRow[], after: OutlineRow[], tr: Translate): OutlineRow[] {
    const was = new Map(before.map(row => [row.id, row]));
    const now = new Set(after.map(row => row.id));
    return [
        ...after.map((row) => {
            const old = was.get(row.id);
            if (!old) {
                return addedRow(row);
            }
            if (row.body?.kind === 'table' && old.body?.kind === 'table') {
                const table = compareTable(old.body.entries, row.body.entries);
                return table ? { ...row, lead: table.lead, body: { kind: 'table' as const, entries: table.entries }, change: CHANGED } : row;
            }
            if (row.body?.kind === 'rows' && old.body?.kind === 'rows') {
                const tools = compareTools(old.body.rows, row.body.rows, tr);
                return tools ? { ...row, lead: tools.lead, body: { kind: 'rows' as const, rows: tools.rows }, change: CHANGED } : row;
            }
            return row;
        }),
        ...before.filter(row => !now.has(row.id)).map(removedRow),
    ];
}
