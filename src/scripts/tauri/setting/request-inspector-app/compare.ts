import type { ExchangeView, Translate } from './RequestInspectorContract';
import { compareHead } from './compare-head';
import { UNCHANGED_LABEL, addedRow, changedRow, content, removedRow } from './compare-row';
import { LEAD_LENGTH, clip, fill } from './format';
import {
    type Assembly, type Outline, type OutlineFold, type OutlineItem, type OutlineMessage, type OutlineRow,
    FOLD_OVER, buildOutline, requestMessages,
} from './outline';
import { changedRanges, diffLines, diffSequences } from './text-diff';

export type Comparable = { view: ExchangeView; assembly: Assembly | null };

export type Comparison = {
    outline: Outline;
    added: number;
    removed: number;
    changed: number;
    same: number;
    /** Estimated tokens over the base; `null` unless both requests carry an estimate. */
    tokens: number | null;
};

type Status = 'same' | 'added' | 'removed' | 'changed';

type Entry = { message: OutlineMessage; row: OutlineRow };

type Shown = { status: Status; message: OutlineMessage; row: OutlineRow };

const RUN_NAMES = 4;

function signature({ message, row }: Entry): string {
    return `${message.role}\u0000${row.lead}\u0000${content(row)}`;
}

const shownRemoved = ({ message, row }: Entry): Shown => ({ status: 'removed', message, row: removedRow(row) });
const shownAdded = ({ message, row }: Entry): Shown => ({ status: 'added', message, row: addedRow(row) });

/** Runs for every candidate pair, before any line diff: it reads each text once. */
function related(before: string, after: string): boolean {
    const was = before.split('\n');
    const now = after.split('\n');
    const [shorter, longer] = before.length <= after.length ? [was, now] : [now, was];
    const known = new Set(longer);
    const kept = shorter.reduce((sum, line) => known.has(line) ? sum + line.length : sum, 0);
    if (kept * 2 >= Math.min(before.length, after.length)) {
        return true;
    }
    const at = was.findIndex((line, index) => line !== now[index]);
    const [removed, added] = [was[at], now[at]];
    return removed !== undefined && added !== undefined && changedRanges(removed, added) !== null;
}

function samePiece(before: OutlineRow, after: OutlineRow): boolean {
    const key = before.origin?.key ?? null;
    return key !== null && key === (after.origin?.key ?? null);
}

function comparable(before: Entry, after: Entry): boolean {
    const was = before.row.origin?.key ?? null;
    const now = after.row.origin?.key ?? null;
    return before.message.role === after.message.role
        && before.row.body?.kind === after.row.body?.kind
        && (was === null || now === null || was === now);
}

function stretchRows(removed: Entry[], added: Entry[]): Shown[] {
    const changed = new Map<Entry, OutlineRow>();
    const replaced = new Set<Entry>();
    let from = 0;
    for (const entry of added) {
        const at = removed.findIndex((candidate, index) => index >= from && comparable(candidate, entry));
        const candidate = removed[at];
        if (!candidate) {
            continue;
        }
        const was = content(candidate.row);
        const now = content(entry.row);
        if (samePiece(candidate.row, entry.row) || related(was, now)) {
            changed.set(entry, changedRow(entry.row, diffLines(was, now)));
            replaced.add(candidate);
            from = at + 1;
        }
    }
    return [
        ...removed.filter(entry => !replaced.has(entry)).map(shownRemoved),
        ...added.map((entry): Shown => {
            const row = changed.get(entry);
            return row ? { status: 'changed', message: entry.message, row } : shownAdded(entry);
        }),
    ];
}

function alignLayer(before: Entry[], after: Entry[]): Shown[] {
    const steps = diffSequences(before, after, signature);
    const shorter = Math.min(before.length, after.length);
    const kept = steps?.filter(step => step.op === 'same').length ?? 0;
    // A short layer is compared row by row whatever it shares: one edited row is all of a one-row layer.
    if (steps === null || (shorter > FOLD_OVER && kept * 2 < shorter)) {
        return [...before.map(shownRemoved), ...after.map(shownAdded)];
    }

    const shown: Shown[] = [];
    let removed: Entry[] = [];
    let added: Entry[] = [];
    for (const step of steps) {
        if (step.op === 'removed') {
            removed.push(step.before);
        } else if (step.op === 'added') {
            added.push(step.after);
        } else {
            shown.push(...stretchRows(removed, added), { status: 'same', message: step.after.message, row: step.after.row });
            removed = [];
            added = [];
        }
    }
    return [...shown, ...stretchRows(removed, added)];
}

/**
 * The conversation and what a preset places around it are lined up apart. As one
 * sequence, a prompt placed at a depth would read as conversation rows moving
 * when the conversation grows past it.
 */
function alignRows(before: Outline, after: Outline, bySource: boolean): Shown[] {
    const entries = (outline: Outline): Entry[] => requestMessages(outline)
        .flatMap(message => message.rows.map(row => ({ message, row })));
    const conversation = ({ message, row }: Entry) => bySource && row.origin ? row.origin.chat : message.tone !== 'system';
    const base = entries(before);
    const current = entries(after);
    const layer = (inConversation: boolean) => {
        const rows = alignLayer(
            base.filter(entry => conversation(entry) === inConversation),
            current.filter(entry => conversation(entry) === inConversation),
        );
        return { rows, at: 0, last: rows.reduce((last, item, index) => item.status === 'removed' ? last : index, -1) };
    };
    const talk = layer(true);
    const placed = layer(false);

    const rows: Shown[] = [];
    for (const entry of current) {
        const queue = conversation(entry) ? talk : placed;
        let end = queue.at;
        while (queue.rows[end]?.status === 'removed') {
            end += 1;
        }
        end = end === queue.last ? queue.rows.length : end + 1;
        rows.push(...queue.rows.slice(queue.at, end));
        queue.at = end;
    }
    return [...rows, ...talk.rows.slice(talk.at), ...placed.rows.slice(placed.at)];
}

function sum(values: (number | null)[]): number | null {
    const known = values.flatMap(value => value ?? []);
    return known.length > 0 ? known.reduce((total, value) => total + value, 0) : null;
}

/** A fragment has no note and no size of its own, so it gets no line for the whole message. */
function fragments(rows: Shown[]): OutlineMessage[] {
    const messages: OutlineMessage[] = [];
    let owner: OutlineMessage | null = null;
    for (const { message, row } of rows) {
        const last = messages.at(-1);
        if (last && owner === message) {
            last.rows.push(row);
        } else {
            messages.push({
                ...message,
                id: `part:${row.id}`,
                note: message.rows.length === 1 ? message.note : '',
                tokens: null,
                rows: [row],
            });
            owner = message;
        }
    }
    return messages;
}

function runNames(messages: OutlineMessage[], tr: Translate): string {
    const names: string[] = [];
    let chat: number[] = [];
    const closeChat = () => {
        if (chat.length > 0) {
            const from = Math.min(...chat);
            const to = Math.max(...chat);
            names.push(from === to ? fill(tr('Chat #{id}'), { id: from }) : fill(tr('Chat #{from}–#{to}'), { from, to }));
            chat = [];
        }
    };
    for (const message of messages) {
        if (names.length > RUN_NAMES) {
            break;
        }
        if (message.dialogue && message.chatIds.length > 0) {
            chat.push(...message.chatIds);
        } else {
            closeChat();
            names.push(...message.rows.flatMap(row => row.name || []));
        }
    }
    closeChat();
    return clip(names.length > RUN_NAMES ? `${names.slice(0, RUN_NAMES).join(' · ')} …` : names.join(' · '), LEAD_LENGTH);
}

const FOLD_LABELS = {
    same: UNCHANGED_LABEL,
    added: '{count} added',
    removed: '{count} removed',
} as const;

function outlineItems(rows: Shown[], tr: Translate): OutlineItem[] {
    const items: OutlineItem[] = [];
    let listed: Shown[] = [];
    const list = () => {
        items.push(...fragments(listed).map(message => ({ kind: 'message' as const, message })));
        listed = [];
    };
    for (let start = 0; start < rows.length;) {
        const { status } = rows[start] as Shown;
        let end = start + 1;
        while (rows[end]?.status === status) {
            end += 1;
        }
        const run = rows.slice(start, end);
        if (status !== 'changed' && (status === 'same' || run.length > FOLD_OVER)) {
            list();
            const messages = fragments(run);
            const fold: OutlineFold = {
                id: `${status}:${run[0]?.row.id ?? ''}`,
                label: fill(tr(FOLD_LABELS[status]), { count: run.length }),
                note: runNames(messages, tr),
                tokens: sum(run.map(({ row }) => row.tokens)),
                messages,
                change: status,
            };
            items.push({ kind: 'fold', fold });
        } else {
            listed.push(...run);
        }
        start = end;
    }
    list();
    return items;
}

/** Both outlines are built at one granularity: a text is split into its pieces only when both requests know theirs. */
export function compareRequests(base: Comparable, current: Comparable, tr: Translate): Comparison {
    const bySource = base.assembly !== null && current.assembly !== null;
    const before = buildOutline(base.view, base.assembly, tr, bySource);
    const after = buildOutline(current.view, current.assembly, tr, bySource);

    const head = compareHead(before.head, after.head, tr);
    const rows = alignRows(before, after, bySource);
    const items = outlineItems(rows, tr);

    const statuses: Status[] = [...head.map(row => row.change?.kind ?? 'same'), ...rows.map(row => row.status)];
    const tally = (status: Status) => statuses.filter(value => value === status).length;
    const sizes = [
        ...rows.map(({ row }) => row.tokens),
        ...items.map(item => item.kind === 'fold' ? item.fold.tokens : null),
    ];
    return {
        outline: {
            head,
            items,
            response: null,
            unlocated: [],
            tokens: after.tokens,
            messageCount: after.messageCount,
            scale: Math.max(1, ...sizes.map(size => size ?? 0)),
        },
        added: tally('added'),
        removed: tally('removed'),
        changed: tally('changed'),
        same: tally('same'),
        tokens: before.tokens !== null && after.tokens !== null ? after.tokens - before.tokens : null,
    };
}
