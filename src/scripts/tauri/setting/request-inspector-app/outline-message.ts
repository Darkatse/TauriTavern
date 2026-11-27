import { formatBytes } from '../format-bytes.js';
import type { MessageView, PartView, PromptPiece, Translate } from './RequestInspectorContract';
import { fill, firstLine, formatCount } from './format';
import type { Assembly, OutlineMessage, OutlineRow, RowOrigin, TextLabel, Tone } from './outline';
import { type PieceRef, type SourceMap, textPartKey } from './prompt-sources';
import { pieceName, promptName } from './source-names';

const TONES: Record<string, Tone> = {
    system: 'system',
    developer: 'system',
    user: 'user',
    assistant: 'assistant',
    model: 'assistant',
    tool: 'tool',
    function: 'tool',
};

/** What building one message reads. */
export type Context = {
    tr: Translate;
    assembly: Assembly | null;
    located: SourceMap | null;
    toolNames: Map<string, string>;
    /** A text assembled from several pieces gets a row for each. */
    split: boolean;
};

/** A row and the assembled piece its size comes from. */
type Draft = { row: OutlineRow; unit: string | null; unitTokens: number };

/**
 * The rows of one message. Only a request message has sources; the text of a
 * response starts open.
 */
export function buildMessage(
    context: Context,
    side: 'request' | 'response',
    index: number,
    message: Pick<MessageView, 'role' | 'parts'> & { name?: string; path?: string },
    note: string,
): OutlineMessage {
    const { tr } = context;
    const id = `${side}.${index}`;
    const open = side === 'response';
    const located = side === 'request' ? context.located : null;
    const drafts: Draft[] = [];
    const chatIds: number[] = [];
    let placed = false;
    let segments = 0;

    const sourced = (draft: Draft, ref: PieceRef | null): Draft => {
        const info = ref && pieceInfo(context, ref);
        if (!info) {
            return draft;
        }
        const { piece } = info;
        const chat = piece.type === 'chat';
        if (!chat) {
            placed = true;
        } else if (piece.messageId !== undefined) {
            chatIds.push(piece.messageId);
        }
        return {
            row: { ...draft.row, origin: pieceOrigin(piece) },
            unit: `${ref.source}:${ref.piece}`,
            unitTokens: info.tokens,
        };
    };

    message.parts.forEach((part, partIndex) => {
        const rowId = `${id}.${partIndex}`;
        const path = part.path ?? null;
        switch (part.type) {
            case 'text': {
                const pieces = located?.segments.get(textPartKey(index, partIndex)) ?? [];
                segments += pieces.length;
                const whole = draft(textRow(rowId, part.text, path, open));
                let rows: Draft[];
                if (pieces.length === 0) {
                    rows = [whole];
                } else if (pieces.length > 1 && !context.split) {
                    rows = [together(whole, pieces.map(segment => sourced(whole, segment)))];
                } else {
                    rows = pieces.map((segment, at) => {
                        const text = part.text.slice(at === 0 ? 0 : segment.start, pieces[at + 1]?.start);
                        const info = pieceInfo(context, segment);
                        return sourced(draft({
                            ...textRow(`${rowId}.${at}`, text, path, open, attachedLabels(text, info?.piece, context)),
                            name: info?.name ?? '',
                            note: info?.note ?? '',
                        }), segment);
                    });
                }
                if (part.cacheBreakpoint) {
                    rows.at(-1)?.row.tags.push(tr('Cache breakpoint'));
                }
                drafts.push(...rows);
                break;
            }
            case 'reasoning':
                drafts.push(draft({
                    ...textRow(rowId, part.text, path, false),
                    name: tr('Reasoning'),
                    lead: fill(tr('{count} chars'), { count: formatCount(part.text.length) }),
                }));
                break;
            case 'nativeState':
                break;
            case 'media':
                drafts.push(draft({
                    ...emptyRow(rowId, path),
                    name: tr('Media'),
                    lead: [part.mime, part.encodedLength === undefined ? '' : formatBytes(Math.floor(part.encodedLength * 3 / 4))]
                        .filter(Boolean).join(' · '),
                    body: part.url === undefined ? null : { kind: 'text', text: part.url, labels: [] },
                }));
                break;
            case 'toolCall': {
                const source = located?.calls.get(part.id);
                const ref = source === undefined ? null : { source, piece: 0 };
                drafts.push(sourced(draft({
                    ...emptyRow(rowId, path),
                    name: (ref && pieceInfo(context, ref)?.name) ?? '',
                    lead: firstLine(callLead(part.name, part.arguments, part.argumentsText)),
                    code: true,
                    body: part.argumentsText === undefined
                        ? { kind: 'json', value: part.arguments }
                        : { kind: 'text', text: part.argumentsText, labels: [] },
                    tags: part.argumentsText === undefined ? [] : [tr('Not valid JSON')],
                }), ref));
                break;
            }
            case 'toolResult': {
                const source = located?.results.get(part.callId);
                const text = resultText(part.parts);
                drafts.push(sourced(draft({
                    ...textRow(rowId, text, path, false),
                    name: fill(tr('{name} result'), {
                        name: part.name ?? context.toolNames.get(part.callId) ?? part.callId,
                    }),
                    tags: part.isError ? [tr('Error')] : [],
                }), source === undefined ? null : { source, piece: 0 }));
                break;
            }
            case 'other':
                drafts.push(draft({
                    ...emptyRow(rowId, path),
                    name: part.label,
                    lead: firstLine(compact(part.value)),
                    code: true,
                    body: { kind: 'json', value: part.value },
                }));
                break;
        }
    });

    const tone = TONES[message.role] ?? 'system';
    const { rows, tokens } = sizeRows(drafts);
    const first = rows[0];
    if (first && message.name) {
        first.note = [message.name, first.note].filter(Boolean).join(' · ');
    }
    return {
        id,
        role: message.role,
        tone,
        note: note || (segments > 1 ? fill(tr('Merged message · {count}'), { count: segments }) : ''),
        tokens,
        path: message.path ?? null,
        rows,
        dialogue: tone !== 'system' && !placed,
        chatIds,
    };
}

/**
 * A size is shown once. A message assembled from one piece carries it on the
 * message; with several pieces, each piece's first row carries its share.
 */
function sizeRows(drafts: Draft[]): { rows: OutlineRow[]; tokens: number | null } {
    const units = new Map<string, number>();
    for (const { unit, unitTokens } of drafts) {
        if (unit !== null) {
            units.set(unit, unitTokens);
        }
    }
    const tokens = units.size === 0 ? null : [...units.values()].reduce((sum, value) => sum + value, 0);
    const shown = new Set<string>();
    const rows = drafts.map(({ row, unit }) => {
        if (drafts.length === 1) {
            return { ...row, tokens };
        }
        if (unit === null || units.size < 2 || shown.has(unit)) {
            return row;
        }
        shown.add(unit);
        return { ...row, tokens: units.get(unit) ?? null };
    });
    return { rows, tokens };
}

function pieceInfo(context: Context, ref: PieceRef): { piece: PromptPiece; name: string; note: string; tokens: number } | null {
    const source = context.assembly?.promptSources[ref.source];
    const piece = source?.pieces[ref.piece];
    if (!source || !piece || !context.assembly) {
        return null;
    }
    return {
        piece,
        ...pieceName(piece, context.assembly.promptNames, context.tr),
        tokens: source.pieces.length > 1 ? piece.tokens : source.tokens,
    };
}

function pieceOrigin(piece: PromptPiece): RowOrigin {
    const chat = piece.type === 'chat';
    const key = chat ? piece.messageId : piece.identifier;
    return { chat, key: key === undefined ? null : `${piece.type}:${key}` };
}

function together(whole: Draft, pieces: Draft[]): Draft {
    return {
        row: whole.row,
        unit: whole.row.id,
        unitTokens: pieces.reduce((sum, piece) => sum + piece.unitTokens, 0),
    };
}

function attachedLabels(text: string, piece: PromptPiece | undefined, context: Context): TextLabel[] {
    const labels: TextLabel[] = [];
    let from = 0;
    for (const entry of piece?.attached ?? []) {
        const needle = entry.text.trim();
        const offset = needle ? text.indexOf(needle, from) : -1;
        if (offset >= 0) {
            labels.push({ offset, name: promptName(entry.identifier, context.assembly?.promptNames ?? {}) });
            from = offset + needle.length;
        }
    }
    return labels;
}

function draft(row: OutlineRow): Draft {
    return { row, unit: null, unitTokens: 0 };
}

export function emptyRow(id: string, path: string | null): OutlineRow {
    return {
        id, name: '', note: '', lead: '', code: false, body: null, tokens: null, path, tags: [], open: false,
        origin: null, change: null,
    };
}

function textRow(id: string, text: string, path: string | null, open: boolean, labels: TextLabel[] = []): OutlineRow {
    return { ...emptyRow(id, path), lead: firstLine(text), body: { kind: 'text', text, labels }, open };
}

export function promptMessage(prompt: string): OutlineMessage {
    return {
        id: 'prompt',
        role: 'prompt',
        tone: 'system',
        note: '',
        tokens: null,
        path: '/prompt',
        rows: [textRow('prompt.0', prompt, '/prompt', true)],
        dialogue: false,
        chatIds: [],
    };
}

function callLead(name: string, args: unknown, argumentsText: string | undefined): string {
    if (argumentsText !== undefined) {
        return `${name}(${firstLine(argumentsText)})`;
    }
    if (args === null || typeof args !== 'object' || Array.isArray(args)) {
        return `${name}(${args === undefined ? '' : compact(args)})`;
    }
    return `${name}(${Object.entries(args).map(([key, value]) => `${key}: ${compact(value)}`).join(', ')})`;
}

function resultText(parts: PartView[]): string {
    return parts.map(part => {
        switch (part.type) {
            case 'text':
            case 'reasoning': return part.text;
            case 'media': return `[${part.mime}]`;
            case 'other': return compact(part.value);
            default: return '';
        }
    }).filter(Boolean).join('\n');
}

export function compact(value: unknown): string {
    return JSON.stringify(value) ?? '';
}
