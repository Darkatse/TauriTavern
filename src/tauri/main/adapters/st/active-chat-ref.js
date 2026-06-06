// @ts-check

import { stripJsonl } from '../../kernel/chat-utils.js';
import {
    characterStemFromAvatarFileName,
    hasCharacterAvatarIdentity,
} from '../../services/characters/character-identity.js';

/**
 * @typedef {{ kind: 'character'; characterId: string; fileName: string }} CharacterChatRef
 * @typedef {{ kind: 'group'; chatId: string }} GroupChatRef
 * @typedef {CharacterChatRef | GroupChatRef} ChatRef
 *
 * @typedef {{
 *   ref: ChatRef;
 *   windowLength: number;
 * }} ActiveChatSnapshot
 */

/**
 * @returns {any}
 */
export function mustGetSillyTavernContext() {
    const hostWindow = /** @type {any} */ (window);
    const getContext = hostWindow?.SillyTavern?.getContext;
    if (typeof getContext !== 'function') {
        throw new Error('SillyTavern.getContext() is unavailable');
    }

    const context = getContext();
    if (!context || typeof context !== 'object') {
        throw new Error('SillyTavern.getContext() returned invalid context');
    }

    return context;
}

/**
 * @returns {ActiveChatSnapshot}
 */
export function getActiveChatSnapshot() {
    const context = mustGetSillyTavernContext();

    const chat = context.chat;
    if (!Array.isArray(chat)) {
        throw new Error('SillyTavern context chat is not an array');
    }

    const windowLength = chat.length;
    const rawChatId = context.chatId;
    const chatId = stripJsonl(rawChatId);

    if (context.groupId) {
        if (!chatId.trim()) {
            throw new Error('SillyTavern context chatId is empty for group chat');
        }
        return {
            ref: {
                kind: 'group',
                chatId,
            },
            windowLength,
        };
    }

    const characters = context.characters;
    const characterIndex = context.characterId;
    const activeCharacter = Array.isArray(characters) ? characters[Number(characterIndex)] : null;

    const avatar = activeCharacter?.avatar;
    const characterId = hasCharacterAvatarIdentity(avatar)
        ? characterStemFromAvatarFileName(avatar, 'avatar', { required: true })
        : String(activeCharacter?.name || '').trim();

    if (!characterId) {
        throw new Error('Failed to resolve active character id');
    }

    if (!chatId.trim()) {
        throw new Error('SillyTavern context chatId is empty for character chat');
    }

    return {
        ref: {
            kind: 'character',
            characterId,
            fileName: chatId,
        },
        windowLength,
    };
}
