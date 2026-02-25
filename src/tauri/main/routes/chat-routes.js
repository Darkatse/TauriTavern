function normalizePayload(payload) {
    if (!Array.isArray(payload)) {
        return [];
    }

    return payload.filter((entry) => entry && typeof entry === 'object');
}

function payloadToJsonl(payload) {
    return normalizePayload(payload).map((entry) => JSON.stringify(entry)).join('\n');
}

function payloadMessages(payload) {
    const normalized = normalizePayload(payload);
    return normalized.filter((entry, index) => index > 0 && typeof entry?.mes === 'string');
}

function previewMessage(messages) {
    const maxLength = 400;
    const lastMessage = messages[messages.length - 1]?.mes;
    if (!lastMessage || typeof lastMessage !== 'string') {
        return '';
    }

    if (lastMessage.length <= maxLength) {
        return lastMessage;
    }

    return `...${lastMessage.slice(lastMessage.length - maxLength)}`;
}

function lastMessageTimestamp(context, payload) {
    const messages = payloadMessages(payload);
    const lastMessage = messages[messages.length - 1];
    return context.parseTimestamp(lastMessage?.send_date);
}

function searchFragments(query) {
    return String(query || '')
        .trim()
        .toLowerCase()
        .split(/\s+/)
        .filter(Boolean);
}

function matchesSearch(fileStem, payload, query) {
    const fragments = searchFragments(query);
    if (fragments.length === 0) {
        return true;
    }

    const messages = payloadMessages(payload);
    const searchText = [fileStem, ...messages.map((message) => String(message.mes || ''))]
        .join('\n')
        .toLowerCase();

    return fragments.every((fragment) => searchText.includes(fragment));
}

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

export function registerChatRoutes(router, context, { jsonResponse }) {
    const isChatNotFoundError = (error) => {
        const serialized = (() => {
            try {
                return JSON.stringify(error);
            } catch {
                return '';
            }
        })();

        const message = [error?.message, error, serialized]
            .map((value) => String(value || ''))
            .join(' ')
            .toLowerCase();
        return (
            message.includes('not found') ||
            message.includes('no such file') ||
            message.includes('enoent') ||
            message.includes('os error 2') ||
            message.includes('failed to open jsonl file') ||
            message.includes('没有那个文件或目录') ||
            message.includes('找不到')
        );
    };

    const isIntegrityError = (error) => {
        const serialized = (() => {
            try {
                return JSON.stringify(error);
            } catch {
                return '';
            }
        })();

        return [error?.message, error, serialized]
            .map((value) => String(value || '').toLowerCase())
            .join(' ')
            .includes('integrity');
    };

    router.post('/api/chats/get', async ({ body }) => {
        const characterId = await context.resolveCharacterId({
            avatar: body?.avatar_url,
            fallbackName: body?.ch_name || body?.character_name,
        });

        const fileName = context.stripJsonl(body?.file_name || body?.chatfile || body?.file);

        if (!characterId || !fileName) {
            return jsonResponse([]);
        }

        try {
            const payload = await context.safeInvoke('get_chat_payload', {
                character_name: characterId,
                file_name: fileName,
                allow_not_found: true,
            });
            return jsonResponse(Array.isArray(payload) ? payload : []);
        } catch (error) {
            if (isChatNotFoundError(error)) {
                return jsonResponse([]);
            }

            return jsonResponse(
                {
                    error: 'Failed to load chat',
                    details: String(error?.message || error || ''),
                },
                500,
            );
        }
    });

    router.post('/api/chats/save', async ({ body }) => {
        const characterId = await context.resolveCharacterId({
            avatar: body?.avatar_url,
            fallbackName: body?.ch_name || body?.character_name,
        });

        const fileName = context.stripJsonl(body?.file_name || body?.chatfile || body?.file);
        if (!characterId || !fileName || !Array.isArray(body?.chat)) {
            return jsonResponse({ error: 'Invalid chat payload' }, 400);
        }

        try {
            await context.safeInvoke('save_chat', {
                dto: {
                    ch_name: characterId,
                    file_name: fileName,
                    chat: body.chat,
                    force: Boolean(body?.force),
                },
            });
            return jsonResponse({ ok: true });
        } catch (error) {
            if (isIntegrityError(error)) {
                return jsonResponse({ error: 'integrity' }, 400);
            }

            return jsonResponse(
                {
                    error: 'Failed to save chat',
                    details: String(error?.message || error || ''),
                },
                500,
            );
        }
    });

    router.post('/api/chats/delete', async ({ body }) => {
        const characterId = await context.resolveCharacterId({
            avatar: body?.avatar_url,
            fallbackName: body?.ch_name || body?.character_name,
        });

        const fileName = context.stripJsonl(body?.chatfile || body?.file_name || body?.file);
        if (!characterId || !fileName) {
            return jsonResponse({ ok: true });
        }

        await context.safeInvoke('delete_chat', {
            characterName: characterId,
            fileName,
        });

        return jsonResponse({ ok: true });
    });

    router.post('/api/chats/rename', async ({ body }) => {
        const oldFileName = context.stripJsonl(body?.original_file || body?.old_file_name);
        const newFileName = context.stripJsonl(body?.renamed_file || body?.new_file_name);

        if (!oldFileName || !newFileName) {
            return jsonResponse({ error: 'Invalid rename payload' }, 400);
        }

        if (body?.is_group) {
            try {
                await context.safeInvoke('rename_group_chat', {
                    dto: {
                        old_file_name: oldFileName,
                        new_file_name: newFileName,
                    },
                });
                return jsonResponse({ ok: true, sanitizedFileName: newFileName });
            } catch {
                return jsonResponse({ error: true }, 400);
            }
        }

        const characterId = await context.resolveCharacterId({ avatar: body?.avatar_url });
        if (!characterId) {
            return jsonResponse({ error: 'Invalid rename payload' }, 400);
        }

        try {
            await context.safeInvoke('rename_chat', {
                dto: {
                    character_name: characterId,
                    old_file_name: oldFileName,
                    new_file_name: newFileName,
                },
            });
            return jsonResponse({ ok: true, sanitizedFileName: newFileName });
        } catch {
            return jsonResponse({ error: true }, 400);
        }
    });

    router.post('/api/chats/search', async ({ body }) => {
        const query = String(body?.query || '');

        if (body?.group_id) {
            const group = await context.safeInvoke('get_group', { id: String(body.group_id) });
            if (!group || !Array.isArray(group.chats) || group.chats.length === 0) {
                return jsonResponse([]);
            }
            const chatIds = group.chats
                .map((chatId) => String(chatId || '').trim())
                .filter(Boolean);
            if (chatIds.length === 0) {
                return jsonResponse([]);
            }

            const results = await context.safeInvoke('search_group_chats', {
                query,
                chat_ids: chatIds,
            });

            const mapped = Array.isArray(results)
                ? results.map((entry) => ({
                    file_name: context.ensureJsonl(entry.file_name),
                    file_size: context.formatFileSize(entry.file_size),
                    message_count: Number(entry.message_count || 0),
                    preview_message: entry.preview || '',
                    last_mes: Number(entry.date || 0),
                }))
                : [];

            mapped.sort((a, b) => Number(b.last_mes || 0) - Number(a.last_mes || 0));
            return jsonResponse(mapped);
        }

        const characterId = await context.resolveCharacterId({ avatar: body?.avatar_url });
        const results = await context.safeInvoke('search_chats', {
            query,
            characterFilter: characterId || null,
        });

        const mapped = Array.isArray(results)
            ? results.map((entry) => ({
                file_name: context.ensureJsonl(entry.file_name),
                file_size: context.formatFileSize(entry.file_size),
                message_count: Number(entry.message_count || 0),
                preview_message: entry.preview || '',
                last_mes: Number(entry.date || 0),
            }))
            : [];

        return jsonResponse(mapped);
    });

    router.post('/api/chats/recent', async ({ body }) => {
        const pinnedChats = normalizePinnedChats(body?.pinned);
        const requestedMax = Number.parseInt(body?.max, 10);
        const max = (
            Number.isFinite(requestedMax)
                ? Math.max(0, requestedMax)
                : Number.MAX_SAFE_INTEGER
        ) + pinnedChats.length;
        const withMetadata = Boolean(body?.metadata);
        const [chats, groups] = await Promise.all([
            context.safeInvoke('list_chat_summaries', {
                include_metadata: withMetadata,
            }),
            context.safeInvoke('get_all_groups'),
        ]);
        await context.getAllCharacters({ shallow: true });

        const characterEntries = Array.isArray(chats)
            ? chats.map((chat) => {
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

        const groupChatIds = Array.from(groupChatToGroup.keys());
        const groupSummaries = groupChatIds.length > 0
            ? await context.safeInvoke('list_group_chat_summaries', {
                chat_ids: groupChatIds,
                include_metadata: withMetadata,
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

        return jsonResponse(allEntries.slice(0, Math.max(0, max)));
    });

    router.post('/api/chats/export', async ({ body }) => {
        const isGroup = Boolean(body?.is_group);
        const format = String(body?.format || 'txt').toLowerCase();
        const exportFilename = String(body?.exportfilename || '');
        const fileName = context.stripJsonl(body?.file || body?.file_name);

        if (!fileName) {
            return jsonResponse({ message: 'Invalid export payload' }, 400);
        }

        let payload;
        try {
            if (isGroup) {
                payload = await context.safeInvoke('get_group_chat', {
                    dto: { id: fileName },
                });
            } else {
                const characterId = await context.resolveCharacterId({
                    avatar: body?.avatar_url,
                    fallbackName: body?.ch_name,
                });

                if (!characterId) {
                    return jsonResponse({ message: 'Invalid export payload' }, 400);
                }

                payload = await context.safeInvoke('get_chat_payload', {
                    character_name: characterId,
                    file_name: fileName,
                });
            }
        } catch {
            return jsonResponse(
                { message: `Could not find JSONL file to export. Source chat file: ${fileName}.` },
                404,
            );
        }

        const normalizedPayload = normalizePayload(payload);
        const result = format === 'jsonl'
            ? payloadToJsonl(normalizedPayload)
            : context.exportChatAsText(normalizedPayload);

        return jsonResponse({
            message: exportFilename ? `Chat saved to ${exportFilename}` : 'Chat exported',
            result,
        });
    });

    router.post('/api/chats/import', async ({ body }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        const file = body.get('avatar');
        if (!(file instanceof Blob)) {
            return jsonResponse({ error: 'No chat file provided' }, 400);
        }

        const fileType = String(body.get('file_type') || '').trim().toLowerCase();
        if (!['json', 'jsonl'].includes(fileType)) {
            return jsonResponse({ error: true });
        }

        const characterDisplayName = String(body.get('character_name') || '').trim();
        const characterId = await context.resolveCharacterId({
            avatar: body.get('avatar_url'),
            fallbackName: characterDisplayName,
        });
        if (!characterId) {
            return jsonResponse({ error: true }, 400);
        }

        const preferredName = file instanceof File && file.name ? file.name : `import.${fileType}`;
        const fileInfo = await context.materializeUploadFile(file, {
            preferredName,
            preferredExtension: fileType,
        });
        if (!fileInfo?.filePath) {
            const reason = fileInfo?.error ? `: ${fileInfo.error}` : '';
            return jsonResponse({ error: `Unable to access uploaded chat file path${reason}` }, 400);
        }

        try {
            const fileNames = await context.safeInvoke('import_character_chats', {
                dto: {
                    character_name: characterId,
                    character_display_name: characterDisplayName || null,
                    user_name: String(body.get('user_name') || '').trim() || null,
                    file_path: fileInfo.filePath,
                    file_type: fileType,
                },
            });

            return jsonResponse({
                res: true,
                fileNames: Array.isArray(fileNames) ? fileNames : [],
            });
        } catch {
            return jsonResponse({ error: true });
        } finally {
            await fileInfo.cleanup?.();
        }
    });

    router.post('/api/chats/group/get', async ({ body }) => {
        const id = String(body?.id || '').trim();
        if (!id) {
            return jsonResponse([], 400);
        }

        try {
            const payload = await context.safeInvoke('get_group_chat', {
                dto: { id },
                allow_not_found: true,
            });
            return jsonResponse(Array.isArray(payload) ? payload : []);
        } catch (error) {
            if (isChatNotFoundError(error)) {
                return jsonResponse([]);
            }
            return jsonResponse([], 500);
        }
    });

    router.post('/api/chats/group/save', async ({ body }) => {
        const id = String(body?.id || '').trim();
        if (!id || !Array.isArray(body?.chat)) {
            return jsonResponse({ error: 'Invalid group chat payload' }, 400);
        }

        try {
            await context.safeInvoke('save_group_chat', {
                dto: {
                    id,
                    chat: body.chat,
                    force: Boolean(body?.force),
                },
            });
            return jsonResponse({ ok: true });
        } catch (error) {
            if (isIntegrityError(error)) {
                return jsonResponse({ error: 'integrity' }, 400);
            }

            return jsonResponse(
                {
                    error: 'Failed to save group chat',
                    details: String(error?.message || error || ''),
                },
                500,
            );
        }
    });

    router.post('/api/chats/group/delete', async ({ body }) => {
        const id = String(body?.id || '').trim();
        if (!id) {
            return jsonResponse({ error: true }, 400);
        }

        try {
            await context.safeInvoke('delete_group_chat', {
                dto: { id },
            });
            return jsonResponse({ ok: true });
        } catch {
            return jsonResponse({ error: true }, 400);
        }
    });

    router.post('/api/chats/group/import', async ({ body }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        const file = body.get('avatar');
        if (!(file instanceof Blob)) {
            return jsonResponse({ error: true }, 400);
        }

        const preferredName = file instanceof File && file.name ? file.name : 'group-chat.jsonl';
        const fileInfo = await context.materializeUploadFile(file, {
            preferredName,
            preferredExtension: 'jsonl',
        });
        if (!fileInfo?.filePath) {
            const reason = fileInfo?.error ? `: ${fileInfo.error}` : '';
            return jsonResponse({ error: `Unable to access uploaded group chat file path${reason}` }, 400);
        }

        try {
            const chatId = await context.safeInvoke('import_group_chat_payload', {
                dto: { file_path: fileInfo.filePath },
            });
            return jsonResponse({ res: String(chatId || '') });
        } catch {
            return jsonResponse({ error: true });
        } finally {
            await fileInfo.cleanup?.();
        }
    });
}
