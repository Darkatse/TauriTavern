import type { PromptPiece, Translate } from './RequestInspectorContract';
import { fill } from './format';

/** Extension prompt keys with a name of their own in the app. */
const EXTENSION_NAMES: Record<string, string> = {
    '1_memory': 'Summary',
    '2_floating_prompt': "Author's Note",
    '3_vectors': 'Vector Storage',
    '4_vectors_data_bank': 'Data Bank',
    chromadb: 'Smart Context',
    PERSONA_DESCRIPTION: 'Persona Description',
};

const EXTENSION_NAME_PREFIXES: [prefix: string, name: string][] = [
    ['customDepthWI', 'World Info'],
    ['DEPTH_PROMPT', "Character's Note"],
];

export type PieceName = { name: string; note: string };

/**
 * A Prompt Manager prompt keeps the name the Prompt Manager shows; a chat
 * record is named by its message id.
 */
export function pieceName(piece: PromptPiece, promptNames: Record<string, string>, tr: Translate): PieceName {
    const notes: string[] = [];
    let name = '';
    if (piece.type === 'chat') {
        name = piece.messageId === undefined ? '' : fill(tr('Chat #{id}'), { id: piece.messageId });
    } else if (piece.type === 'prompt') {
        name = promptName(piece.identifier, promptNames);
    } else {
        const key = piece.identifier ?? '';
        const known = EXTENSION_NAMES[key] ?? EXTENSION_NAME_PREFIXES.find(([prefix]) => key.startsWith(prefix))?.[1];
        name = known ? tr(known) : key;
        if (!known) {
            notes.push(tr('Extension'));
        }
    }
    if (piece.depth !== undefined) {
        notes.push(fill(tr('Depth {depth}'), { depth: piece.depth }));
    }
    const attached = (piece.attached ?? []).map(entry => promptName(entry.identifier, promptNames));
    if (attached.length > 0) {
        notes.push(fill(tr('with {names}'), { names: attached.join(', ') }));
    }
    return { name, note: notes.join(' · ') };
}

export function promptName(identifier: string | undefined, promptNames: Record<string, string>): string {
    return identifier === undefined ? '' : promptNames[identifier] ?? identifier;
}
