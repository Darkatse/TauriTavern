import type { ExchangeView, RequestPreview, Translate } from './RequestInspectorContract';
import type { DiffLine, TextRange } from './text-diff';
import { fill, firstLine } from './format';
import { type Context, buildMessage, emptyRow, promptMessage } from './outline-message';
import { SYSTEM_MESSAGE, locateSources } from './prompt-sources';
import { pieceName, promptName } from './source-names';

export type Tone = 'system' | 'user' | 'assistant' | 'tool';

/** A name shown inside a row's text, where an attached entry starts. */
export type TextLabel = { offset: number; name: string };

export type ValueChange = { kind: 'added' } | { kind: 'removed' } | { kind: 'changed'; before: unknown };

export type TableEntry = { key: string; value: unknown; change: ValueChange | null };

export type RowBody =
    | { kind: 'text'; text: string; labels: TextLabel[] }
    | { kind: 'json'; value: unknown }
    | { kind: 'table'; entries: TableEntry[] }
    | { kind: 'tool'; description: string; schema: unknown }
    | { kind: 'rows'; rows: OutlineRow[] }
    | { kind: 'diff'; lines: DiffLine[] };

/** Offsets into the row's `lead`: the mark holds only while that lead does. */
export type LeadMark = TextRange & { op: 'added' | 'removed' };

export type RowChange =
    | { kind: 'added' }
    | { kind: 'removed' }
    | { kind: 'changed'; added: number; removed: number; mark: LeadMark | null };

/** `key` names a piece, not a row: the text and the tool calls of one chat record share it. */
export type RowOrigin = { chat: boolean; key: string | null };

/** One line of the outline: something with a name that can be opened. */
export type OutlineRow = {
    id: string;
    name: string;
    note: string;
    lead: string;
    /** The lead is code, not prose. */
    code: boolean;
    body: RowBody | null;
    tokens: number | null;
    /** JSON Pointer of the part in the request or response body. */
    path: string | null;
    tags: string[];
    /** Open before the reader touches it. */
    open: boolean;
    origin: RowOrigin | null;
    change: RowChange | null;
};

export type OutlineMessage = {
    id: string;
    role: string;
    tone: Tone;
    note: string;
    tokens: number | null;
    path: string | null;
    rows: OutlineRow[];
    /** Part of the conversation, as opposed to a prompt placed by the preset. */
    dialogue: boolean;
    chatIds: number[];
};

export type OutlineFold = {
    id: string;
    label: string;
    note: string;
    tokens: number | null;
    messages: OutlineMessage[];
    change: 'same' | 'added' | 'removed' | null;
};

export type OutlineItem =
    | { kind: 'message'; message: OutlineMessage }
    | { kind: 'fold'; fold: OutlineFold };

export type Outline = {
    /** Parameters and tool definitions. */
    head: OutlineRow[];
    items: OutlineItem[];
    response: { finishReason: string | null; messages: OutlineMessage[] } | null;
    unlocated: { name: string; tokens: number }[];
    tokens: number | null;
    messageCount: number;
    /** The largest row, which the size bars are drawn against. */
    scale: number;
};

/** What a preview knows about how the request was assembled. */
export type Assembly = Pick<RequestPreview, 'promptSources' | 'toolSources' | 'promptNames' | 'chatHistory'>;

export const FOLD_OVER = 6;
const FOLD_KEEP = 2;
const HEAD_PARAMETERS = 4;

export function buildOutline(view: ExchangeView, assembly: Assembly | null, tr: Translate, split = true): Outline {
    const { request, response } = view;
    const context: Context = {
        tr,
        assembly,
        split,
        located: assembly ? locateSources(request, assembly.promptSources) : null,
        toolNames: toolNames(view),
    };

    const messages: OutlineMessage[] = [];
    if (request.system.length > 0) {
        messages.push(buildMessage(context, 'request', SYSTEM_MESSAGE, {
            role: 'system',
            parts: request.system,
            ...(request.systemPath === undefined ? {} : { path: request.systemPath }),
        }, request.systemField ?? ''));
    }
    request.messages.forEach((message, index) => {
        messages.push(buildMessage(context, 'request', index, message, ''));
    });
    if (request.prompt !== undefined) {
        messages.push(promptMessage(request.prompt));
    }

    const items = foldDialogue(messages, context);
    const answers = (response?.candidates ?? []).map((candidate, index) => buildMessage(
        context,
        'response',
        index,
        candidate,
        response && response.candidates.length > 1 ? `#${index + 1}` : '',
    ));

    const sizes = [
        ...messages.flatMap(message => message.rows.map(row => row.tokens)),
        ...items.map(item => item.kind === 'fold' ? item.fold.tokens : null),
    ];
    return {
        head: [parametersRow(view, tr), toolsRow(view, context)].filter(row => row !== null),
        items,
        response: response ? { finishReason: response.finishReason ?? null, messages: answers } : null,
        unlocated: unlocatedSources(context),
        tokens: assembly ? assembly.promptSources.reduce((sum, source) => sum + source.tokens, 0) : null,
        messageCount: messages.length,
        scale: Math.max(1, ...sizes.map(size => size ?? 0)),
    };
}

function foldDialogue(messages: OutlineMessage[], context: Context): OutlineItem[] {
    const items: OutlineItem[] = [];
    let run: OutlineMessage[] = [];
    const flush = () => {
        const folded = run.length > FOLD_OVER ? run.slice(0, -FOLD_KEEP) : [];
        if (folded.length > 0) {
            const earliest = !items.some(item => item.kind === 'fold');
            items.push({ kind: 'fold', fold: makeFold(folded, earliest, context) });
        }
        items.push(...run.slice(folded.length).map(message => ({ kind: 'message' as const, message })));
        run = [];
    };
    for (const message of messages) {
        if (message.dialogue) {
            run.push(message);
        } else {
            flush();
            items.push({ kind: 'message', message });
        }
    }
    flush();
    return items;
}

function makeFold(messages: OutlineMessage[], earliest: boolean, { tr, assembly }: Context): OutlineFold {
    const count = fill(tr('{count} messages'), { count: messages.length });
    const chatIds = messages.flatMap(message => message.chatIds);
    const sizes = messages.flatMap(message => message.tokens ?? []);
    const excluded = assembly && earliest ? assembly.chatHistory.candidates - assembly.chatHistory.included : 0;
    return {
        id: `fold:${messages[0]?.id ?? ''}`,
        label: chatIds.length > 0
            ? fill(tr('Chat #{from}–#{to}'), { from: Math.min(...chatIds), to: Math.max(...chatIds) })
            : count,
        note: [
            chatIds.length > 0 ? count : '',
            excluded > 0 ? fill(tr('{count} more left out'), { count: excluded }) : '',
        ].filter(Boolean).join(' · '),
        tokens: sizes.length > 0 ? sizes.reduce((sum, value) => sum + value, 0) : null,
        messages,
        change: null,
    };
}

function parametersRow({ request }: ExchangeView, tr: Translate): OutlineRow | null {
    const entries: TableEntry[] = [
        ...request.parameters.map(({ key, value }) => ({ key, value, change: null })),
        ...(request.toolChoice === undefined ? [] : [{ key: tr('Tool choice'), value: request.toolChoice, change: null }]),
    ];
    if (entries.length === 0) {
        return null;
    }
    return {
        ...emptyRow('parameters', null),
        name: tr('Parameters'),
        lead: firstLine(entries
            .filter(({ value }) => value === null || typeof value !== 'object')
            .slice(0, HEAD_PARAMETERS)
            .map(({ key, value }) => `${key} ${String(value)}`)
            .join(' · ')),
        body: { kind: 'table', entries },
    };
}

function toolsRow({ request }: ExchangeView, { tr, assembly }: Context): OutlineRow | null {
    if (request.tools.length === 0) {
        return null;
    }
    const origin = (name: string): string => {
        const source = assembly?.toolSources.find(tool => tool.name === name);
        switch (source?.type) {
            case 'prompt': return promptName(source.identifier, assembly?.promptNames ?? {});
            case 'mcp': return ['MCP', source.server].filter(Boolean).join(' · ');
            case 'function': return tr('Function tool');
            default: return '';
        }
    };
    return {
        ...emptyRow('tools', null),
        name: fill(tr('Tools {count}'), { count: request.tools.length }),
        lead: firstLine(request.tools.map(tool => tool.name).join(' · ')),
        body: {
            kind: 'rows',
            rows: request.tools.map((tool, index) => ({
                ...emptyRow(`tools.${index}`, tool.path),
                name: tool.name,
                note: origin(tool.name),
                lead: firstLine(tool.description ?? ''),
                body: { kind: 'tool', description: tool.description ?? '', schema: tool.parameters },
            })),
        },
    };
}

function unlocatedSources({ located, assembly, tr }: Context): { name: string; tokens: number }[] {
    return (located?.unlocated ?? []).flatMap(ref => {
        const source = assembly?.promptSources[ref.source];
        const piece = source?.pieces[ref.piece];
        if (!source || !piece || !assembly) {
            return [];
        }
        const { name } = pieceName(piece, assembly.promptNames, tr);
        return [{ name: name || source.role, tokens: source.pieces.length > 1 ? piece.tokens : source.tokens }];
    });
}

function toolNames(view: ExchangeView): Map<string, string> {
    const names = new Map<string, string>();
    const messages = [...view.request.messages, ...(view.response?.candidates ?? [])];
    for (const part of messages.flatMap(message => message.parts)) {
        if (part.type === 'toolCall') {
            names.set(part.id, part.name);
        }
    }
    return names;
}

export function requestMessages(outline: Outline): OutlineMessage[] {
    return outline.items.flatMap(item => item.kind === 'fold' ? item.fold.messages : [item.message]);
}

function eachRow(outline: Outline, visit: (row: OutlineRow) => void): void {
    const walk = (row: OutlineRow) => {
        visit(row);
        if (row.body?.kind === 'rows') {
            row.body.rows.forEach(walk);
        }
    };
    outline.head.forEach(walk);
    for (const message of [...requestMessages(outline), ...(outline.response?.messages ?? [])]) {
        message.rows.forEach(walk);
    }
}

/** The rows open before the reader touches anything. */
export function defaultOpenRows(outline: Outline): Set<string> {
    const ids = new Set<string>();
    eachRow(outline, row => row.open && ids.add(row.id));
    return ids;
}

/** Every row and fold that can be opened. */
export function openableRows(outline: Outline): Set<string> {
    const ids = new Set(outline.items.flatMap(item => item.kind === 'fold' ? [item.fold.id] : []));
    eachRow(outline, row => row.body !== null && ids.add(row.id));
    return ids;
}

export function changedRows(outline: Outline): Set<string> {
    const ids = new Set<string>();
    eachRow(outline, row => row.change !== null && row.body !== null && ids.add(row.id));
    return ids;
}
