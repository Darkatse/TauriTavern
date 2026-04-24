// @ts-check

/**
 * @typedef {{ kind: 'character'; characterId: string; fileName: string }} CharacterChatRef
 * @typedef {{ kind: 'group'; chatId: string }} GroupChatRef
 * @typedef {CharacterChatRef | GroupChatRef} ChatRef
 */

/**
 * @param {any[]} lines
 * @returns {any[]}
 */
export function parseJsonLines(lines) {
    return lines.map((line) => JSON.parse(String(line)));
}

/**
 * @template T
 * @param {unknown} value
 * @param {string} label
 * @returns {T[]}
 */
export function mustArray(value, label) {
    if (!Array.isArray(value)) {
        throw new Error(`${label} must be an array`);
    }
    return /** @type {T[]} */ (value);
}

/**
 * @param {unknown} value
 * @param {string} label
 * @returns {number}
 */
export function mustNumber(value, label) {
    const num = typeof value === 'number' ? value : Number(value);
    if (!Number.isFinite(num)) {
        throw new Error(`${label} must be a finite number`);
    }
    return num;
}

/** @param {unknown} value */
function stripJsonl(value) {
    return String(value || '').trim().replace(/\.jsonl$/i, '');
}

/**
 * @param {any} ref
 * @returns {ChatRef}
 */
export function normalizeChatRef(ref) {
    if (!ref || typeof ref !== 'object') {
        throw new Error('ChatRef must be an object');
    }

    const kind = String(ref.kind || '').trim();
    if (kind === 'character') {
        const characterId = String(ref.characterId || '').trim();
        const fileName = stripJsonl(ref.fileName);
        if (!characterId || !fileName) {
            throw new Error('Character ChatRef requires characterId and fileName');
        }
        return { kind: 'character', characterId, fileName };
    }

    if (kind === 'group') {
        const chatId = stripJsonl(ref.chatId);
        if (!chatId) {
            throw new Error('Group ChatRef requires chatId');
        }
        return { kind: 'group', chatId };
    }

    throw new Error(`Unsupported ChatRef kind: ${kind}`);
}

