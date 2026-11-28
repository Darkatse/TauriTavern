import { expect, test } from '@rstest/core';

import type { PromptSource, RequestView } from './RequestInspectorContract';
import { type Comparable, compareRequests } from './compare';
import { requestMessages } from './outline';

const tr = (key: string) => key;
const text = (value: string) => ({ type: 'text' as const, text: value, cacheBreakpoint: false });

type Piece = { role: string; text: string; name?: string; chat?: number; depth?: number };

const chat = (id: number, value = `line ${id}`): Piece => ({ role: id % 2 ? 'assistant' : 'user', text: value, chat: id });
const prompt = (name: string, value: string, depth?: number): Piece => ({
    role: 'system', text: value, name, ...(depth === undefined ? {} : { depth }),
});

/** Each message is given as the pieces merged into it. */
function request(messages: Piece[][], assembled: boolean): Comparable {
    const view: RequestView = {
        kind: 'chat',
        system: [],
        messages: messages.map(pieces => ({ role: pieces[0]?.role ?? '', parts: [text(pieces.map(piece => piece.text).join('\n'))] })),
        tools: [],
        parameters: [],
    };
    const promptSources: PromptSource[] = messages.flat().map(piece => ({
        role: piece.role,
        tokens: 1,
        pieces: [piece.chat === undefined
            ? { type: 'prompt', identifier: piece.name ?? '', text: piece.text, tokens: 1, ...(piece.depth === undefined ? {} : { depth: piece.depth }) }
            : { type: 'chat', messageId: piece.chat, text: piece.text, tokens: 1 }],
    }));
    const names = Object.fromEntries(messages.flat().flatMap(piece => piece.name ? [[piece.name, piece.name]] : []));
    return {
        view: { request: view },
        assembly: assembled
            ? { promptSources, toolSources: [], promptNames: names, chatHistory: { included: 0, candidates: 0 } }
            : null,
    };
}

function differing(base: Comparable, compared: Comparable): string[] {
    const { outline } = compareRequests(base, compared, tr);
    const signs = { added: '+', removed: '-', changed: '~' };
    return requestMessages(outline).flatMap(message => message.rows)
        .flatMap(row => row.change ? [`${signs[row.change.kind]} ${row.name || row.lead}`] : []);
}

test('a conversation that grew past a prompt placed at a depth only gains its new messages', () => {
    const note = prompt('note', 'Stay in the scene.', 2);
    const turn = (assembled: boolean, last: number) => {
        const messages = Array.from({ length: last + 1 }, (_, id) => [chat(id)]);
        messages.splice(messages.length - 2, 0, [note]);
        return request([[prompt('main', 'You are a narrator.')], ...messages], assembled);
    };

    expect(differing(turn(true, 4), turn(true, 6))).toEqual(['+ Chat #5', '+ Chat #6']);
    expect(differing(turn(false, 4), turn(false, 6))).toEqual(['+ line 5', '+ line 6']);

    const { added, removed, changed, same } = compareRequests(turn(true, 4), turn(true, 6), tr);
    expect({ added, removed, changed, same }).toEqual({ added: 2, removed: 0, changed: 0, same: 7 });
});

test('another conversation is replaced as a whole, though its messages have the same ids', () => {
    const conversation = (word: string, length: number) => request(
        [[prompt('main', 'You are a narrator.')], ...Array.from({ length }, (_, id) => [chat(id, `${word} ${id}`)])],
        true,
    );
    const { added, removed, changed, same } = compareRequests(conversation('harbour', 8), conversation('lighthouse', 12), tr);
    expect({ added, removed, changed, same }).toEqual({ added: 12, removed: 8, changed: 0, same: 1 });
});

test('pieces of a merged message are compared one by one, and only when both requests know them', () => {
    const merged = (...pieces: Piece[]) => [pieces, [chat(0)]];
    const base = merged(prompt('main', 'You are a narrator.'), prompt('style', 'Keep it short.'), prompt('rules', 'Never decide for the user.'));
    const edited = merged(prompt('main', 'You are a narrator.'), prompt('style', 'Keep it very short.'));

    // The newline that joined `style` to `rules` is no change to `style`.
    expect(differing(request(base, true), request(merged(...base[0]!.slice(0, 2)), true))).toEqual(['- rules']);
    expect(differing(request(base, true), request(edited, true))).toEqual(['- rules', '~ style']);

    const { outline, changed } = compareRequests(request(base, false), request(edited, true), tr);
    const rows = requestMessages(outline).flatMap(message => message.rows).filter(row => row.change);
    expect(changed).toBe(1);
    expect(rows.map(row => row.change)).toEqual([{ kind: 'changed', added: 1, removed: 2, mark: { op: 'added', start: 8, end: 12 } }]);
    expect(rows[0]?.lead.slice(8, 12)).toBe('very');
    expect(rows[0]?.lead).toBe('Keep it very short.');
});
