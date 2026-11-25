import type { Outline, OutlineItem, OutlineMessage, OutlineRow } from './outline';
import { countMatches, matchingLine } from './search';

/** The text a row is searched in, apart from its name and note. */
export function rowText(row: OutlineRow): string {
    switch (row.body?.kind) {
        case 'text': return row.body.text;
        case 'json': return `${row.lead}\n${JSON.stringify(row.body.value, null, 1) ?? ''}`;
        case 'table': return row.body.entries.map(({ key, value }) => `${key} ${JSON.stringify(value) ?? ''}`).join('\n');
        case 'tool': return `${row.body.description}\n${JSON.stringify(row.body.schema, null, 1) ?? ''}`;
        case 'diff': return row.body.lines.map(line => line.text).join('\n');
        default: return row.lead;
    }
}

/**
 * Keeps the rows a query matches. Folds are opened up, since a fold would hide
 * what was found; a kept row leads with the line that matched.
 */
export function filterOutline(outline: Outline, needle: string): { outline: Outline; count: number } {
    let count = 0;

    const filterRow = (row: OutlineRow): OutlineRow | null => {
        const text = rowText(row);
        const own = countMatches(row.name, needle) + countMatches(row.note, needle) + countMatches(text, needle);
        const nested = row.body?.kind === 'rows' ? row.body.rows.flatMap(child => filterRow(child) ?? []) : [];
        if (own === 0 && nested.length === 0) {
            return null;
        }
        count += own;
        return {
            ...row,
            lead: matchingLine(text, needle) ?? row.lead,
            ...(row.body?.kind === 'rows' ? { body: { kind: 'rows', rows: nested } } : {}),
        };
    };

    const filterMessage = (message: OutlineMessage): OutlineMessage[] => {
        const rows = message.rows.flatMap(row => filterRow(row) ?? []);
        return rows.length > 0 ? [{ ...message, rows }] : [];
    };

    const items: OutlineItem[] = outline.items
        .flatMap(item => item.kind === 'fold' ? item.fold.messages : [item.message])
        .flatMap(filterMessage)
        .map(message => ({ kind: 'message', message }));

    return {
        outline: {
            ...outline,
            head: outline.head.flatMap(row => filterRow(row) ?? []),
            items,
            response: outline.response && {
                ...outline.response,
                messages: outline.response.messages.flatMap(filterMessage),
            },
        },
        count,
    };
}
