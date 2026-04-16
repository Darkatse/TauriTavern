// @ts-check

import { getActiveChatSnapshot } from '../adapters/st/active-chat-ref.js';
import { getChatHistoryBootstrapModeName } from '../services/chat-history/chat-history-mode-state.js';
import { createChatSearchMessages } from './chat-search-messages.js';
import { mustArray, mustNumber, normalizeChatRef, parseJsonLines } from './chat-utils.js';

/**
 * @typedef {{ kind: 'character'; characterId: string; fileName: string }} CharacterChatRef
 * @typedef {{ kind: 'group'; chatId: string }} GroupChatRef
 * @typedef {CharacterChatRef | GroupChatRef} ChatRef
 *
 * @typedef {{
 *   startIndex: number;
 *   totalCount: number;
 *   messages: any[];
 *   cursor: any;
 *   hasMoreBefore: boolean;
 * }} ChatHistoryPage
 */

/**
 * @param {{ safeInvoke: (command: any, args?: any) => Promise<any>; ref: ChatRef }} deps
 */
function createChatHandle({ safeInvoke, ref }) {
    const normalized = normalizeChatRef(ref);
    const searchMessages = createChatSearchMessages({ safeInvoke, normalized });

    async function summary(options = {}) {
        const includeMetadata = Boolean(options?.includeMetadata);
        if (normalized.kind === 'character') {
            return safeInvoke('get_character_chat_summary', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                includeMetadata,
            });
        }

        return safeInvoke('get_group_chat_summary', {
            chatId: normalized.chatId,
            includeMetadata,
        });
    }

    async function getMetadata() {
        if (normalized.kind === 'character') {
            return safeInvoke('get_character_chat_metadata', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
            });
        }

        return safeInvoke('get_group_chat_metadata', { chatId: normalized.chatId });
    }

    async function setMetadataExtension(options) {
        const namespace = String(options?.namespace || '').trim();
        if (!namespace) {
            throw new Error('namespace is required');
        }

        const value = options?.value;

        if (normalized.kind === 'character') {
            return safeInvoke('set_character_chat_metadata_extension', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                namespace,
                value,
            });
        }

        return safeInvoke('set_group_chat_metadata_extension', {
            chatId: normalized.chatId,
            namespace,
            value,
        });
    }

    async function getStoreJson(options) {
        const namespace = String(options?.namespace || '').trim();
        const key = String(options?.key || '').trim();
        if (!namespace || !key) {
            throw new Error('namespace and key are required');
        }

        if (normalized.kind === 'character') {
            return safeInvoke('get_character_chat_store_json', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                namespace,
                key,
            });
        }

        return safeInvoke('get_group_chat_store_json', {
            chatId: normalized.chatId,
            namespace,
            key,
        });
    }

    async function setStoreJson(options) {
        const namespace = String(options?.namespace || '').trim();
        const key = String(options?.key || '').trim();
        if (!namespace || !key) {
            throw new Error('namespace and key are required');
        }

        const value = options?.value;

        if (normalized.kind === 'character') {
            return safeInvoke('set_character_chat_store_json', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                namespace,
                key,
                value,
            });
        }

        return safeInvoke('set_group_chat_store_json', {
            chatId: normalized.chatId,
            namespace,
            key,
            value,
        });
    }

    async function updateStoreJson(options) {
        const namespace = String(options?.namespace || '').trim();
        const key = String(options?.key || '').trim();
        if (!namespace || !key) {
            throw new Error('namespace and key are required');
        }

        const value = options?.value;

        if (normalized.kind === 'character') {
            return safeInvoke('update_character_chat_store_json', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                namespace,
                key,
                value,
            });
        }

        return safeInvoke('update_group_chat_store_json', {
            chatId: normalized.chatId,
            namespace,
            key,
            value,
        });
    }

    async function renameStoreKey(options) {
        const namespace = String(options?.namespace || '').trim();
        const key = String(options?.key || '').trim();
        const newKey = String(options?.newKey || '').trim();
        if (!namespace || !key || !newKey) {
            throw new Error('namespace, key, and newKey are required');
        }

        if (normalized.kind === 'character') {
            return safeInvoke('rename_character_chat_store_key', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                namespace,
                key,
                newKey,
            });
        }

        return safeInvoke('rename_group_chat_store_key', {
            chatId: normalized.chatId,
            namespace,
            key,
            newKey,
        });
    }

    async function deleteStoreJson(options) {
        const namespace = String(options?.namespace || '').trim();
        const key = String(options?.key || '').trim();
        if (!namespace || !key) {
            throw new Error('namespace and key are required');
        }

        if (normalized.kind === 'character') {
            return safeInvoke('delete_character_chat_store_json', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                namespace,
                key,
            });
        }

        return safeInvoke('delete_group_chat_store_json', {
            chatId: normalized.chatId,
            namespace,
            key,
        });
    }

    async function listStoreKeys(options) {
        const namespace = String(options?.namespace || '').trim();
        if (!namespace) {
            throw new Error('namespace is required');
        }

        if (normalized.kind === 'character') {
            return safeInvoke('list_character_chat_store_keys', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                namespace,
            });
        }

        return safeInvoke('list_group_chat_store_keys', {
            chatId: normalized.chatId,
            namespace,
        });
    }

    async function findLastMessage(query) {
        const payload = query ?? {};

        if (normalized.kind === 'character') {
            return safeInvoke('find_last_character_chat_message', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                query: payload,
            });
        }

        return safeInvoke('find_last_group_chat_message', {
            chatId: normalized.chatId,
            query: payload,
        });
    }

    async function stableId() {
        if (normalized.kind === 'group') {
            return normalized.chatId;
        }

        const metadata = await getMetadata();
        const value = String(metadata?.integrity || '').trim();
        if (!value) {
            throw new Error('Chat metadata integrity is missing');
        }
        return value;
    }

    async function historyTail(options = {}) {
        const limit = Number(options?.limit || 0);
        if (!Number.isFinite(limit) || limit <= 0) {
            throw new Error('limit must be greater than 0');
        }

        const summaryResult = await summary({ includeMetadata: false });
        const totalCount = mustNumber(summaryResult?.message_count, 'summary.message_count');

        let tail;
        if (normalized.kind === 'character') {
            tail = await safeInvoke('get_chat_payload_tail', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                maxLines: limit,
            });
        } else {
            tail = await safeInvoke('get_group_chat_payload_tail', {
                id: normalized.chatId,
                maxLines: limit,
            });
        }

        const messages = parseJsonLines(mustArray(tail?.lines, 'tail.lines'));
        const startIndex = totalCount - messages.length;

        return {
            startIndex,
            totalCount,
            messages,
            cursor: tail.cursor,
            hasMoreBefore: Boolean(tail.hasMoreBefore),
        };
    }

    /**
     * @param {ChatHistoryPage} page
     * @param {{ limit: number }} options
     * @returns {Promise<ChatHistoryPage>}
     */
    async function historyBefore(page, options) {
        if (!page || typeof page !== 'object') {
            throw new Error('page is required');
        }

        const limit = Number(options?.limit || 0);
        if (!Number.isFinite(limit) || limit <= 0) {
            throw new Error('limit must be greater than 0');
        }

        const cursor = page.cursor;
        if (!cursor) {
            throw new Error('page.cursor is required');
        }

        let chunk;
        if (normalized.kind === 'character') {
            chunk = await safeInvoke('get_chat_payload_before', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                cursor,
                maxLines: limit,
            });
        } else {
            chunk = await safeInvoke('get_group_chat_payload_before', {
                id: normalized.chatId,
                cursor,
                maxLines: limit,
            });
        }

        const messages = parseJsonLines(mustArray(chunk?.lines, 'chunk.lines'));
        const startIndex = Number(page.startIndex) - messages.length;

        return {
            startIndex,
            totalCount: Number(page.totalCount),
            messages,
            cursor: chunk.cursor,
            hasMoreBefore: Boolean(chunk.hasMoreBefore),
        };
    }

    /**
     * @param {ChatHistoryPage} page
     * @param {{ limit: number; pages: number }} options
     */
    async function historyBeforePages(page, options) {
        if (!page || typeof page !== 'object') {
            throw new Error('page is required');
        }

        const limit = Number(options?.limit || 0);
        const pages = Number(options?.pages || 0);
        if (!Number.isFinite(limit) || limit <= 0) {
            throw new Error('limit must be greater than 0');
        }
        if (!Number.isFinite(pages) || pages <= 0) {
            throw new Error('pages must be greater than 0');
        }

        const cursor = page.cursor;
        if (!cursor) {
            throw new Error('page.cursor is required');
        }

        let chunks;
        if (normalized.kind === 'character') {
            chunks = await safeInvoke('get_chat_payload_before_pages', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                cursor,
                maxLines: limit,
                maxPages: pages,
            });
        } else {
            chunks = await safeInvoke('get_group_chat_payload_before_pages', {
                id: normalized.chatId,
                cursor,
                maxLines: limit,
                maxPages: pages,
            });
        }

        const results = [];
        let nextStartIndex = Number(page.startIndex);

        for (const chunk of mustArray(chunks, 'chunks')) {
            const messages = parseJsonLines(mustArray(chunk?.lines, 'chunk.lines'));
            nextStartIndex -= messages.length;
            results.push({
                startIndex: nextStartIndex,
                totalCount: Number(page.totalCount),
                messages,
                cursor: chunk.cursor,
                hasMoreBefore: Boolean(chunk.hasMoreBefore),
            });
        }

        return results;
    }

    return {
        ref: normalized,
        summary,
        stableId,
        searchMessages,
        metadata: {
            get: getMetadata,
            setExtension: setMetadataExtension,
        },
        store: {
            getJson: getStoreJson,
            setJson: setStoreJson,
            updateJson: updateStoreJson,
            updateJSON: updateStoreJson,
            renameKey: renameStoreKey,
            updateKey: renameStoreKey,
            deleteJson: deleteStoreJson,
            listKeys: listStoreKeys,
        },
        locate: {
            findLastMessage,
        },
        history: {
            tail: historyTail,
            before: historyBefore,
            beforePages: historyBeforePages,
        },
    };
}

/**
 * @param {{ safeInvoke: (command: any, args?: any) => Promise<any> }} deps
 */
function createChatApi({ safeInvoke }) {
    function open(ref) {
        return createChatHandle({ safeInvoke, ref });
    }

    const current = {
        ref() {
            return getActiveChatSnapshot().ref;
        },
        handle() {
            return open(getActiveChatSnapshot().ref);
        },
        async windowInfo() {
            const { ref, windowLength } = getActiveChatSnapshot();
            const handle = open(ref);
            const summaryResult = await handle.summary({ includeMetadata: false });
            const totalCount = mustNumber(summaryResult?.message_count, 'summary.message_count');
            const windowStartIndex = totalCount - windowLength;
            const mode = getChatHistoryBootstrapModeName();
            return {
                mode,
                chatKind: ref.kind,
                chatRef: ref,
                totalCount,
                windowStartIndex,
                windowLength,
            };
        },
    };

    return { open, current };
}

/**
 * @param {any} context
 */
export function installChatApi(context) {
    const hostWindow = /** @type {any} */ (window);
    const hostAbi = hostWindow.__TAURITAVERN__;
    if (!hostAbi || typeof hostAbi !== 'object') {
        throw new Error('Host ABI __TAURITAVERN__ is missing');
    }

    const safeInvoke = context?.safeInvoke;
    if (typeof safeInvoke !== 'function') {
        throw new Error('Tauri main context safeInvoke is missing');
    }

    if (!hostAbi.api || typeof hostAbi.api !== 'object') {
        hostAbi.api = {};
    }

    hostAbi.api.chat = createChatApi({ safeInvoke });
}
