import { resolveRouteCharacterId } from './character-route-utils.js';

export function registerChatImportRoutes(router, context, { jsonResponse }) {
    router.post('/api/chats/import', async ({ body }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        const backupName = String(body.get('backup_name') || '').trim();
        const file = body.get('avatar');
        const restoreFromBackup = Boolean(backupName) && !(file instanceof Blob);
        if (!restoreFromBackup && !(file instanceof Blob)) {
            return jsonResponse({ error: 'No chat file provided' }, 400);
        }

        const fileType = String(body.get('file_type') || '').trim().toLowerCase();
        if (!restoreFromBackup && !['json', 'jsonl'].includes(fileType)) {
            return jsonResponse({ error: true });
        }

        const characterDisplayName = String(body.get('character_name') || '').trim();
        const resolved = await resolveRouteCharacterId(context, {
            avatar: body.get('avatar_url'),
            fallbackName: characterDisplayName,
        });
        if (resolved.responseBody) {
            return jsonResponse(resolved.responseBody, 400);
        }
        const characterId = resolved.characterId;
        if (!characterId) {
            return jsonResponse({ error: true }, 400);
        }

        if (restoreFromBackup) {
            try {
                const fileNames = await context.safeInvoke('restore_character_chat_backup', {
                    dto: {
                        backup_name: backupName,
                        character_name: characterId,
                        character_display_name: characterDisplayName || characterId,
                    },
                });

                return jsonResponse({
                    res: true,
                    fileNames: Array.isArray(fileNames) ? fileNames : [],
                });
            } catch {
                return jsonResponse({ error: true });
            }
        }

        const preferredName = file instanceof File && file.name ? file.name : `import.${fileType}`;
        const fileInfo = await context.materializeUploadFile(file, {
            kind: 'chat-import',
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

    router.post('/api/chats/group/import', async ({ body }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        const backupName = String(body.get('backup_name') || '').trim();
        const file = body.get('avatar');
        const restoreFromBackup = Boolean(backupName) && !(file instanceof Blob);
        if (restoreFromBackup) {
            try {
                const chatId = await context.safeInvoke('restore_group_chat_backup', {
                    dto: { backup_name: backupName },
                });
                return jsonResponse({ res: String(chatId || '') });
            } catch {
                return jsonResponse({ error: true });
            }
        }

        if (!(file instanceof Blob)) {
            return jsonResponse({ error: true }, 400);
        }

        const preferredName = file instanceof File && file.name ? file.name : 'group-chat.jsonl';
        const fileInfo = await context.materializeUploadFile(file, {
            kind: 'chat-import',
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
