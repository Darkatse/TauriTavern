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

            const mapped = await Promise.all(
                group.chats.map(async (chatId) => {
                    const id = String(chatId || '').trim();
                    if (!id) {
                        return null;
                    }

                    try {
                        const payload = await context.safeInvoke('get_group_chat', { dto: { id } });
                        if (!Array.isArray(payload)) {
                            return null;
                        }

                        if (!matchesSearch(id, payload, query)) {
                            return null;
                        }

                        const messages = payloadMessages(payload);
                        return {
                            file_name: context.ensureJsonl(id),
                            file_size: context.formatFileSize(new TextEncoder().encode(payloadToJsonl(payload)).length),
                            message_count: messages.length,
                            preview_message: previewMessage(messages),
                            last_mes: lastMessageTimestamp(context, payload),
                        };
                    } catch {
                        return null;
                    }
                }),
            );

            const results = mapped.filter(Boolean);
            results.sort((a, b) => Number(b.last_mes || 0) - Number(a.last_mes || 0));
            return jsonResponse(results);
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
        const max = Number(body?.max || Number.MAX_SAFE_INTEGER);
        const withMetadata = Boolean(body?.metadata);
        const chats = await context.safeInvoke('get_all_chats');
        const groups = await context.safeInvoke('get_all_groups');
        await context.getAllCharacters({ shallow: false });

        const characterEntries = Array.isArray(chats)
            ? chats.map((chat) => {
                const frontendChat = context.toFrontendChat(chat);
                const messageCount = Math.max(0, frontendChat.length - 1);
                const lastMessage = frontendChat[frontendChat.length - 1] || {};
                const avatar = context.findAvatarByCharacterId(chat.character_name);
                const result = {
                    file_name: context.ensureJsonl(chat.file_name || ''),
                    file_size: '0 B',
                    chat_items: messageCount,
                    mes: lastMessage.mes || '',
                    last_mes: context.parseTimestamp(lastMessage.send_date),
                    avatar: avatar || '',
                };

                if (withMetadata) {
                    result.chat_metadata = frontendChat[0]?.chat_metadata || {};
                }

                return result;
            })
            : [];

        const groupEntries = Array.isArray(groups)
            ? await Promise.all(
                groups.flatMap((group) => {
                    const groupId = String(group?.id || '').trim();
                    const chatIds = Array.isArray(group?.chats) ? group.chats : [];

                    return chatIds.map(async (chatId) => {
                        const id = String(chatId || '').trim();
                        if (!id) {
                            return null;
                        }

                        try {
                            const payload = await context.safeInvoke('get_group_chat', { dto: { id } });
                            if (!Array.isArray(payload)) {
                                return null;
                            }

                            const messages = payloadMessages(payload);
                            const header = payload[0] && typeof payload[0] === 'object' ? payload[0] : {};
                            const lastMessage = messages[messages.length - 1] || {};
                            const result = {
                                file_name: context.ensureJsonl(id),
                                file_size: context.formatFileSize(new TextEncoder().encode(payloadToJsonl(payload)).length),
                                chat_items: messages.length,
                                mes: lastMessage?.mes || '',
                                last_mes: context.parseTimestamp(lastMessage?.send_date),
                                group: groupId,
                            };

                            if (withMetadata) {
                                result.chat_metadata = header?.chat_metadata || {};
                            }

                            return result;
                        } catch {
                            return null;
                        }
                    });
                }),
            )
            : [];

        const allEntries = [...characterEntries, ...groupEntries.filter(Boolean)];
        allEntries.sort((a, b) => Number(b.last_mes || 0) - Number(a.last_mes || 0));

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
