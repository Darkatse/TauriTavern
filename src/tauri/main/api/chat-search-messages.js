// @ts-check

/**
 * @param {{ safeInvoke: (command: any, args?: any) => Promise<any>; normalized: any }} deps
 */
export function createChatSearchMessages({ safeInvoke, normalized }) {
    return async function searchMessages(options = {}) {
        const query = String(options?.query || '').trim();
        if (!query) {
            throw new Error('query is required');
        }

        const limit = Number(options?.limit ?? 20);
        if (!Number.isFinite(limit) || limit <= 0) {
            throw new Error('limit must be greater than 0');
        }

        const payload = {
            query,
            limit,
            filters: options?.filters,
        };

        if (normalized.kind === 'character') {
            return safeInvoke('search_character_chat_messages', {
                characterName: normalized.characterId,
                fileName: normalized.fileName,
                query: payload,
            });
        }

        return safeInvoke('search_group_chat_messages', {
            chatId: normalized.chatId,
            query: payload,
        });
    };
}

