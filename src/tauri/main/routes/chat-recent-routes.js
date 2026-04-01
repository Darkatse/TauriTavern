function normalizePinnedChats(value) {
    if (!Array.isArray(value)) {
        return [];
    }

    return value
        .filter((entry) => entry && typeof entry === 'object')
        .map((entry) => ({
            file_name: String(entry.file_name || ''),
            avatar: String(entry.avatar || ''),
            group: String(entry.group || ''),
        }))
        .filter((entry) => entry.file_name);
}

function isPinnedRecentChat(chat, pinnedChats) {
    return pinnedChats.some((pinned) =>
        pinned.file_name === String(chat?.file_name || '')
        && pinned.avatar === String(chat?.avatar || '')
        && pinned.group === String(chat?.group || ''),
    );
}

export function registerChatRecentRoutes(router, context, { jsonResponse }) {
    router.post('/api/chats/recent', async ({ body }) => {
        try {
            const pinnedChats = normalizePinnedChats(body?.pinned);
            const requestedMax = Number.parseInt(body?.max, 10);
            const requestedRecentLimit = (
                Number.isFinite(requestedMax)
                    ? Math.max(0, requestedMax)
                    : Number.MAX_SAFE_INTEGER
            );
            const responseLimit = requestedRecentLimit + pinnedChats.length;
            const withMetadata = Boolean(body?.metadata);
            const [groups] = await Promise.all([
                context.safeInvoke('get_all_groups'),
                context.getAllCharacters({ shallow: true }),
            ]);

            const groupChatToGroup = new Map();
            if (Array.isArray(groups)) {
                groups.forEach((group) => {
                    const groupId = String(group?.id || '').trim();
                    const chatIds = Array.isArray(group?.chats) ? group.chats : [];
                    if (!groupId) {
                        return;
                    }

                    chatIds.forEach((chatId) => {
                        const id = context.stripJsonl(chatId);
                        if (!id || groupChatToGroup.has(id)) {
                            return;
                        }
                        groupChatToGroup.set(id, groupId);
                    });
                });
            }

            const pinnedCharacterRefs = [];
            const pinnedCharacterRefKeys = new Set();
            await Promise.all(pinnedChats.map(async (chat) => {
                const avatar = String(chat?.avatar || '').trim();
                const fileStem = context.stripJsonl(chat?.file_name || '');
                if (!avatar || !fileStem || chat?.group) {
                    return;
                }

                const characterId = await context.resolveCharacterId({ avatar });
                if (!characterId) {
                    return;
                }

                const key = `${characterId}/${fileStem}`;
                if (pinnedCharacterRefKeys.has(key)) {
                    return;
                }
                pinnedCharacterRefKeys.add(key);
                pinnedCharacterRefs.push({
                    character_name: characterId,
                    file_name: fileStem,
                });
            }));

            const pinnedGroupRefs = [];
            const pinnedGroupRefKeys = new Set();
            pinnedChats.forEach((chat) => {
                const groupId = String(chat?.group || '').trim();
                const fileStem = context.stripJsonl(chat?.file_name || '');
                if (!groupId || !fileStem) {
                    return;
                }

                if (groupChatToGroup.get(fileStem) !== groupId) {
                    return;
                }

                if (pinnedGroupRefKeys.has(fileStem)) {
                    return;
                }
                pinnedGroupRefKeys.add(fileStem);
                pinnedGroupRefs.push({ chat_id: fileStem });
            });

            const characterQueryLimit = requestedRecentLimit + pinnedCharacterRefs.length;
            const groupChatIds = Array.from(groupChatToGroup.keys());
            const groupQueryLimit = requestedRecentLimit + pinnedGroupRefs.length;
            const [characterSummaries, groupSummaries] = await Promise.all([
                context.safeInvoke('list_recent_chat_summaries', {
                    include_metadata: withMetadata,
                    max_entries: characterQueryLimit,
                    pinned: pinnedCharacterRefs,
                }),
                groupChatIds.length > 0
                    ? context.safeInvoke('list_recent_group_chat_summaries', {
                        chat_ids: groupChatIds,
                        include_metadata: withMetadata,
                        max_entries: groupQueryLimit,
                        pinned: pinnedGroupRefs,
                    })
                    : Promise.resolve([]),
            ]);

            const characterEntries = Array.isArray(characterSummaries)
                ? characterSummaries.map((chat) => {
                    const characterId = String(chat?.character_name || '').trim();
                    const fileStem = context.stripJsonl(chat?.file_name || '');
                    if (!characterId || !fileStem) {
                        return null;
                    }

                    const avatar = context.findAvatarByCharacterId(characterId);
                    const result = {
                        file_name: context.ensureJsonl(chat.file_name || ''),
                        file_size: context.formatFileSize(chat.file_size),
                        chat_items: Number(chat.message_count || 0),
                        mes: String(chat.preview || ''),
                        last_mes: Number(chat.date || 0),
                        avatar: avatar || '',
                    };

                    if (withMetadata) {
                        result.chat_metadata = chat?.chat_metadata || {};
                    }

                    return result;
                })
                : [];

            const groupEntries = Array.isArray(groupSummaries)
                ? groupSummaries.map((chat) => {
                    const fileName = context.ensureJsonl(chat.file_name || '');
                    const fileStem = context.stripJsonl(fileName);
                    const groupId = groupChatToGroup.get(fileStem);
                    if (!groupId) {
                        return null;
                    }

                    const result = {
                        file_name: fileName,
                        file_size: context.formatFileSize(chat.file_size),
                        chat_items: Number(chat.message_count || 0),
                        mes: String(chat.preview || ''),
                        last_mes: Number(chat.date || 0),
                        group: groupId,
                    };

                    if (withMetadata) {
                        result.chat_metadata = chat?.chat_metadata || {};
                    }

                    return result;
                })
                : [];

            const allEntries = [...characterEntries.filter(Boolean), ...groupEntries.filter(Boolean)];
            allEntries.sort((a, b) => {
                const aPinned = isPinnedRecentChat(a, pinnedChats);
                const bPinned = isPinnedRecentChat(b, pinnedChats);
                if (aPinned && !bPinned) {
                    return -1;
                }
                if (!aPinned && bPinned) {
                    return 1;
                }

                return Number(b.last_mes || 0) - Number(a.last_mes || 0);
            });

            return jsonResponse(allEntries.slice(0, Math.max(0, responseLimit)));
        } catch (error) {
            return jsonResponse(
                {
                    error: 'Failed to load recent chats',
                    details: String(error?.message || error || ''),
                },
                500,
            );
        }
    });
}

