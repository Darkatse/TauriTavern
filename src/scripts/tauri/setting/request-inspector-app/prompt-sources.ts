import type { PartView, PromptSource, RequestView } from './RequestInspectorContract';

/** A piece of a source message, by index into the assembly's sources. */
export type PieceRef = { source: number; piece: number };

export type TextSegment = PieceRef & { start: number };

export type SourceMap = {
    /** Where each piece starts inside a text part, in order. Keyed by `textPartKey`. */
    segments: Map<string, TextSegment[]>;
    /** The source message of a tool call and of a tool result, by call id. */
    calls: Map<string, number>;
    results: Map<string, number>;
    unlocated: PieceRef[];
};

/** Message index of the top-level system field. */
export const SYSTEM_MESSAGE = -1;

export function textPartKey(message: number, part: number): string {
    return `${message}:${part}`;
}

/**
 * Finds where each assembled piece ended up in the wire request.
 *
 * Wire conversion only adds or removes text around a piece (merged system
 * messages, name prefixes, same-role merging), so pieces are searched in send
 * order with a cursor that never moves back. Tool turns are matched by call id.
 */
export function locateSources(request: RequestView, sources: PromptSource[]): SourceMap {
    const texts = textParts(request);
    const { calls, results } = toolIds(request);
    const map: SourceMap = { segments: new Map(), calls: new Map(), results: new Map(), unlocated: [] };
    let part = 0;
    let offset = 0;

    sources.forEach((source, sourceIndex) => {
        if (source.toolCallId !== undefined) {
            if (results.has(source.toolCallId)) {
                map.results.set(source.toolCallId, sourceIndex);
            } else {
                map.unlocated.push({ source: sourceIndex, piece: 0 });
            }
            return;
        }

        const callIds = source.toolCallIds ?? [];
        let located = false;
        for (const id of callIds) {
            if (calls.has(id)) {
                map.calls.set(id, sourceIndex);
                located = true;
            }
        }

        source.pieces.forEach((piece, pieceIndex) => {
            const needle = piece.text.trim();
            if (!needle) {
                return;
            }
            for (let index = part; index < texts.length; index++) {
                const text = texts[index];
                const start = text ? text.text.indexOf(needle, index === part ? offset : 0) : -1;
                if (text && start >= 0) {
                    const segments = map.segments.get(text.key) ?? [];
                    segments.push({ source: sourceIndex, piece: pieceIndex, start });
                    map.segments.set(text.key, segments);
                    part = index;
                    offset = start + needle.length;
                    located = true;
                    return;
                }
            }
            map.unlocated.push({ source: sourceIndex, piece: pieceIndex });
        });

        if (!located && callIds.length > 0 && source.pieces.every(piece => !piece.text.trim())) {
            map.unlocated.push({ source: sourceIndex, piece: 0 });
        }
    });

    return map;
}

function textParts(request: RequestView): { key: string; text: string }[] {
    const collect = (message: number, parts: PartView[]) => parts.flatMap((part, index) => part.type === 'text'
        ? [{ key: textPartKey(message, index), text: part.text }]
        : []);
    return [
        ...collect(SYSTEM_MESSAGE, request.system),
        ...request.messages.flatMap((message, index) => collect(index, message.parts)),
    ];
}

function toolIds(request: RequestView): { calls: Set<string>; results: Set<string> } {
    const calls = new Set<string>();
    const results = new Set<string>();
    for (const part of request.messages.flatMap(message => message.parts)) {
        if (part.type === 'toolCall') {
            calls.add(part.id);
        } else if (part.type === 'toolResult') {
            results.add(part.callId);
        }
    }
    return { calls, results };
}
