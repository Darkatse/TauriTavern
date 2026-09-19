import { MacroRegistry, MacroCategory, MacroValueType } from '../engine/MacroRegistry.js';
import { chat, chat_metadata } from '../../../script.js';
import { findLastMessageId, getLastSwipeId, getCurrentSwipeId } from '../chat-state.js';

/**
 * Registers macros that inspect the current chat log and swipe state
 * (message texts, indices, swipes, and context boundaries).
 */
export function registerChatMacros() {
    MacroRegistry.registerMacro('lastMessage', {
        category: MacroCategory.CHAT,
        description: 'Last message in the chat.',
        returns: 'Last message in the chat.',
        handler: () => String(getLastMessage() ?? ''),
    });

    MacroRegistry.registerMacro('lastMessageId', {
        category: MacroCategory.CHAT,
        description: 'Index of the last message in the chat.',
        returns: 'Index of the last message in the chat.',
        returnType: MacroValueType.INTEGER,
        handler: () => String(findLastMessageId(chat) ?? ''),
    });

    MacroRegistry.registerMacro('lastUserMessage', {
        category: MacroCategory.CHAT,
        description: 'Last user message in the chat.',
        returns: 'Last user message in the chat.',
        handler: () => String(getLastUserMessage() ?? ''),
    });

    MacroRegistry.registerMacro('lastCharMessage', {
        category: MacroCategory.CHAT,
        description: 'Last character/bot message in the chat.',
        returns: 'Last character/bot message in the chat.',
        handler: () => String(getLastCharMessage() ?? ''),
    });

    MacroRegistry.registerMacro('firstIncludedMessageId', {
        category: MacroCategory.CHAT,
        description: 'Index of the first message included in the current context.',
        returns: 'Index of the first message included in the context.',
        returnType: MacroValueType.INTEGER,
        handler: () => String(getFirstIncludedMessageId() ?? ''),
    });

    MacroRegistry.registerMacro('firstDisplayedMessageId', {
        category: MacroCategory.CHAT,
        description: 'Index of the first displayed message in the chat.',
        returns: 'Index of the first displayed message in the chat.',
        returnType: MacroValueType.INTEGER,
        handler: () => String(getFirstDisplayedMessageId() ?? ''),
    });

    MacroRegistry.registerMacro('lastSwipeId', {
        category: MacroCategory.CHAT,
        description: 'Number of existing swipes for the last message, including a pending swipe\'s message.',
        returns: 'Number of existing swipes.',
        returnType: MacroValueType.INTEGER,
        handler: () => String(getLastSwipeId(chat) ?? ''),
    });

    MacroRegistry.registerMacro('currentSwipeId', {
        category: MacroCategory.CHAT,
        description: '1-based index of the current swipe.',
        returns: '1-based index of the current swipe.',
        returnType: MacroValueType.INTEGER,
        handler: () => String(getCurrentSwipeId(chat) ?? ''),
    });

    MacroRegistry.registerMacro('allChatRange', {
        category: MacroCategory.CHAT,
        description: 'Range of all message IDs in the chat (e.g. "0-10"). Empty string if the chat is empty.',
        returns: 'Range string from 0 to last message ID, or empty string.',
        handler: () => {
            if (!Array.isArray(chat) || chat.length === 0) {
                return '';
            }
            return `0-${chat.length - 1}`;
        },
    });
}

function getLastMessage() {
    const mid = findLastMessageId(chat);
    return typeof mid === 'number' ? (chat[mid]?.mes ?? '') : '';
}

function getLastUserMessage() {
    const mid = findLastMessageId(chat, { filter: m => Boolean(m.is_user && !m.is_system) });
    return typeof mid === 'number' ? (chat[mid]?.mes ?? '') : '';
}

function getLastCharMessage() {
    const mid = findLastMessageId(chat, { filter: m => !m.is_user && !m.is_system });
    return typeof mid === 'number' ? (chat[mid]?.mes ?? '') : '';
}

function getFirstIncludedMessageId() {
    const value = chat_metadata.lastInContextMessageId;
    return typeof value === 'number' ? value : null;
}

function getFirstDisplayedMessageId() {
    const mesElement = document.querySelector('#chat .mes');
    const mesId = Number(mesElement?.getAttribute('mesid'));
    if (!Number.isNaN(mesId) && mesId >= 0) {
        return mesId;
    }
    return null;
}
