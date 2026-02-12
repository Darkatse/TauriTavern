export function registerCharacterRoutes(router, context, { jsonResponse, textResponse }) {
    router.post('/api/characters/all', async () => {
        const characters = await context.getAllCharacters({ shallow: false, forceRefresh: true });
        return jsonResponse(characters);
    });

    router.post('/api/characters/get', async ({ body }) => {
        const character = await context.getSingleCharacter(body);
        if (!character) {
            return jsonResponse({ error: 'Character not found' }, 404);
        }

        return jsonResponse(character);
    });

    router.post('/api/characters/chats', async ({ body }) => {
        const avatar = body?.avatar_url || body?.avatar;
        const simple = Boolean(body?.simple);
        const characterId = await context.resolveCharacterId({ avatar, fallbackName: body?.ch_name || body?.name });

        if (!characterId) {
            return jsonResponse([]);
        }

        const chats = await context.safeInvoke('get_character_chats_by_id', {
            dto: {
                name: characterId,
                simple,
            },
        });

        const mapped = Array.isArray(chats)
            ? chats.map((chat) => ({
                file_name: context.ensureJsonl(chat.file_name),
                file_size: chat.file_size,
                chat_items: Number(chat.chat_items || 0),
                message_count: Number(chat.chat_items || 0),
                last_message: chat.last_message,
                preview_message: chat.last_message,
                last_mes: chat.last_message_date,
            }))
            : [];

        return jsonResponse(mapped);
    });

    router.post('/api/characters/create', async ({ body, url }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        const created = await context.createCharacterFromForm(body, url);
        await context.getAllCharacters({ shallow: false, forceRefresh: true });
        return textResponse(created?.avatar || '');
    });

    router.post('/api/characters/edit', async ({ body, url }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        await context.editCharacterFromForm(body, url);
        await context.getAllCharacters({ shallow: false, forceRefresh: true });
        return textResponse('ok');
    });

    router.post('/api/characters/delete', async ({ body }) => {
        const avatar = body?.avatar_url;
        const characterId = await context.resolveCharacterId({ avatar, fallbackName: body?.name });

        if (!characterId) {
            return jsonResponse({ error: 'Character not found' }, 404);
        }

        await context.safeInvoke('delete_character', {
            dto: {
                name: characterId,
                delete_chats: Boolean(body?.delete_chats),
            },
        });

        await context.getAllCharacters({ shallow: false, forceRefresh: true });
        return jsonResponse({ ok: true });
    });

    router.post('/api/characters/rename', async ({ body }) => {
        const avatar = body?.avatar_url;
        const newName = body?.new_name || '';
        const oldCharacterId = await context.resolveCharacterId({ avatar });

        if (!oldCharacterId || !newName) {
            return jsonResponse({ error: 'Character rename payload is invalid' }, 400);
        }

        const renamed = await context.safeInvoke('rename_character', {
            dto: {
                old_name: oldCharacterId,
                new_name: newName,
            },
        });

        const normalized = context.normalizeCharacter(renamed);
        await context.getAllCharacters({ shallow: false, forceRefresh: true });
        return jsonResponse(normalized);
    });

    router.post('/api/characters/duplicate', async ({ body }) => {
        const avatar = body?.avatar_url;
        const originalCharacterId = await context.resolveCharacterId({ avatar });

        if (!originalCharacterId) {
            return jsonResponse({ error: 'Character not found' }, 404);
        }

        const original = await context.safeInvoke('get_character', { name: originalCharacterId });
        const baseName = `${original.name} (Copy)`;
        const duplicateName = await context.uniqueCharacterName(baseName);

        const dto = {
            name: duplicateName,
            description: original.description || '',
            personality: original.personality || '',
            scenario: original.scenario || '',
            first_mes: original.first_mes || '',
            mes_example: original.mes_example || '',
            creator: original.creator || '',
            creator_notes: original.creator_notes || '',
            character_version: original.character_version || '',
            tags: Array.isArray(original.tags) ? original.tags : [],
            talkativeness: Number(original.talkativeness ?? 0.5),
            fav: Boolean(original.fav),
            alternate_greetings: Array.isArray(original.alternate_greetings) ? original.alternate_greetings : [],
            system_prompt: original.system_prompt || '',
            post_history_instructions: original.post_history_instructions || '',
            extensions: context.normalizeExtensions(original.extensions),
        };

        const created = await context.safeInvoke('create_character', { dto });
        const normalized = context.normalizeCharacter(created);
        await context.getAllCharacters({ shallow: false, forceRefresh: true });

        return jsonResponse({ path: normalized.avatar });
    });

    router.post('/api/characters/merge-attributes', async ({ body }) => {
        const avatar = body?.avatar;
        const characterId = await context.resolveCharacterId({ avatar, fallbackName: body?.name });

        if (!characterId) {
            return jsonResponse({ ok: true });
        }

        const dto = context.pickCharacterUpdateFields(body || {});
        if (Object.keys(dto).length > 0) {
            await context.safeInvoke('update_character', { name: characterId, dto });
            await context.getAllCharacters({ shallow: false, forceRefresh: true });
        }

        return jsonResponse({ ok: true });
    });

    router.post('/api/characters/import', async ({ body }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        const file = body.get('avatar');
        if (!(file instanceof Blob)) {
            return jsonResponse({ error: 'No character file provided' }, 400);
        }

        const fileType = String(body.get('file_type') || '').trim().toLowerCase();
        const fallbackName = fileType ? `import.${fileType}` : 'import.bin';
        const preferredName = file instanceof File && file.name ? file.name : fallbackName;

        const fileInfo = await context.materializeUploadFile(file, {
            preferredName,
            preferredExtension: fileType,
        });
        if (!fileInfo?.filePath) {
            const reason = fileInfo?.error ? `: ${fileInfo.error}` : '';
            return jsonResponse({ error: `Unable to access uploaded character file path${reason}` }, 400);
        }

        const preserveFileName = body.get('preserved_name');

        let imported;
        try {
            imported = await context.safeInvoke('import_character', {
                dto: {
                    file_path: fileInfo.filePath,
                    preserve_file_name: preserveFileName ? String(preserveFileName) : null,
                },
            });
        } finally {
            await fileInfo.cleanup?.();
        }

        const normalized = context.normalizeCharacter(imported);
        await context.getAllCharacters({ shallow: false, forceRefresh: true });

        return jsonResponse({
            file_name: String(normalized.avatar || '').replace(/\.png$/i, ''),
        });
    });

    router.post('/api/characters/export', async ({ body }) => {
        const avatar = body?.avatar_url;
        const format = String(body?.format || 'json').toLowerCase();
        const characterId = await context.resolveCharacterId({ avatar, fallbackName: body?.name });

        if (!characterId) {
            return jsonResponse({ error: 'Character not found' }, 404);
        }

        const character = await context.safeInvoke('get_character', { name: characterId });
        const payload = JSON.stringify(character, null, 2);
        const contentType = format === 'json' ? 'application/json' : 'application/octet-stream';

        return new Response(payload, { status: 200, headers: { 'Content-Type': contentType } });
    });
}
