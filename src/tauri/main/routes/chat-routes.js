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

    router.post('/api/chats/get', async ({ body }) => {
        const characterId = await context.resolveCharacterId({
            avatar: body?.avatar_url,
            fallbackName: body?.ch_name || body?.character_name,
        });

        const fileName = context.stripJsonl(body?.file_name || body?.chatfile || body?.file);

        if (!characterId || !fileName) {
            return jsonResponse([]);
        }

        let chat;
        try {
            chat = await context.safeInvoke('get_chat', {
                characterName: characterId,
                fileName,
            });
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

        return jsonResponse(context.toFrontendChat(chat));
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

        await context.safeInvoke('save_chat', {
            dto: {
                ch_name: characterId,
                file_name: fileName,
                chat: body.chat,
                force: Boolean(body?.force),
            },
        });

        return jsonResponse({ ok: true });
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
        if (body?.is_group) {
            return jsonResponse({ error: 'Group chat rename is not supported' }, 400);
        }

        const characterId = await context.resolveCharacterId({ avatar: body?.avatar_url });
        const oldFileName = context.stripJsonl(body?.original_file || body?.old_file_name);
        const newFileName = context.stripJsonl(body?.renamed_file || body?.new_file_name);

        if (!characterId || !oldFileName || !newFileName) {
            return jsonResponse({ error: 'Invalid rename payload' }, 400);
        }

        await context.safeInvoke('rename_chat', {
            dto: {
                character_name: characterId,
                old_file_name: oldFileName,
                new_file_name: newFileName,
            },
        });

        return jsonResponse({ ok: true, sanitizedFileName: newFileName });
    });

    router.post('/api/chats/search', async ({ body }) => {
        if (body?.group_id) {
            return jsonResponse([]);
        }

        const characterId = await context.resolveCharacterId({ avatar: body?.avatar_url });
        const query = String(body?.query || '');
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
        const max = Number(body?.max || 20);
        const chats = await context.safeInvoke('get_all_chats');
        await context.getAllCharacters({ shallow: false });

        const mapped = Array.isArray(chats)
            ? chats.map((chat) => {
                const frontChat = context.toFrontendChat(chat);
                const messageCount = Math.max(0, frontChat.length - 1);
                const lastMessage = frontChat[frontChat.length - 1] || {};
                const avatar = context.findAvatarByCharacterId(chat.character_name);

                return {
                    file_name: context.ensureJsonl(chat.file_name || ''),
                    file_size: '0 B',
                    chat_items: messageCount,
                    mes: lastMessage.mes || '',
                    last_mes: context.parseTimestamp(lastMessage.send_date),
                    avatar: avatar || '',
                };
            })
            : [];

        mapped.sort((a, b) => Number(b.last_mes || 0) - Number(a.last_mes || 0));
        return jsonResponse(mapped.slice(0, Math.max(0, max)));
    });

    router.post('/api/chats/export', async ({ body }) => {
        if (body?.is_group) {
            return jsonResponse({ message: 'Group chat export is not supported in Tauri backend' }, 400);
        }

        const characterId = await context.resolveCharacterId({
            avatar: body?.avatar_url,
            fallbackName: body?.ch_name,
        });

        const fileName = context.stripJsonl(body?.file || body?.file_name);
        if (!characterId || !fileName) {
            return jsonResponse({ message: 'Invalid export payload' }, 400);
        }

        const chat = await context.safeInvoke('get_chat', {
            characterName: characterId,
            fileName,
        });

        const frontendChat = context.toFrontendChat(chat);
        const format = String(body?.format || 'txt').toLowerCase();
        const result = format === 'txt'
            ? context.exportChatAsText(frontendChat)
            : context.exportChatAsJsonl(frontendChat);

        return jsonResponse({ message: 'Chat exported', result });
    });

    router.post('/api/chats/import', async () => jsonResponse({ res: false, fileNames: [] }, 400));
    router.post('/api/chats/group/get', async () => jsonResponse([]));
    router.post('/api/chats/group/save', async () => jsonResponse({ ok: true }));
    router.post('/api/chats/group/delete', async () => jsonResponse({ ok: true }));
    router.post('/api/chats/group/import', async () => jsonResponse({ res: false, fileNames: [] }));
}
