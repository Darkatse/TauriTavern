import { normalizeBinaryPayload, sanitizeAttachmentFileName } from '../binary-utils.js';
import { CHARACTER_CREATE_WARNINGS } from '../services/characters/character-create-service.js';
import { assertCharacterAvatarFileName } from '../services/characters/character-identity.js';
import {
    badRequestBody,
    isBadRequestError,
    resolveExistingRouteCharacterId,
    resolveRouteCharacterId,
} from './character-route-utils.js';

const CHARACTER_CREATE_WARNING_HEADER = 'x-tauritavern-warning';

function hasBodyField(body, fieldName) {
    return Boolean(body && typeof body === 'object' && !Array.isArray(body)
        && Object.prototype.hasOwnProperty.call(body, fieldName));
}

function pickAvatarIdentity(body) {
    if (hasBodyField(body, 'avatar_url')) {
        return body.avatar_url;
    }

    if (hasBodyField(body, 'avatar')) {
        return body.avatar;
    }

    return undefined;
}

/** @param {Record<string, any>} body */
function buildCharacterMergeUpdate(body) {
    const update = { ...body };

    if (!Object.prototype.hasOwnProperty.call(update, 'avatar') && update.avatar_url !== undefined) {
        update.avatar = update.avatar_url;
    }

    delete update.avatar_url;
    return update;
}

/**
 * @param {any} character
 * @param {'agentProfiles' | 'skills'} field
 */
function getCharacterTauriExtensionField(character, field) {
    const sources = [
        character?.data?.extensions?.tauritavern,
        character?.extensions?.tauritavern,
    ];

    for (const source of sources) {
        if (source && typeof source === 'object' && !Array.isArray(source)
            && Object.prototype.hasOwnProperty.call(source, field)) {
            return source[field];
        }
    }

    return undefined;
}

/**
 * @param {any} character
 * @param {'agentProfiles' | 'skills'} field
 */
function hasCharacterEmbeddedAgentAsset(character, field) {
    const value = getCharacterTauriExtensionField(character, field);
    return value !== undefined && value !== null;
}

function createCharacterResponse(outcome, textResponse) {
    const response = textResponse(outcome.character?.avatar || '');
    const hasAvatarImportWarning = outcome.warnings?.some(
        (warning) => warning?.code === CHARACTER_CREATE_WARNINGS.AVATAR_IMPORT_FAILED,
    );

    if (hasAvatarImportWarning) {
        response.headers.set(CHARACTER_CREATE_WARNING_HEADER, CHARACTER_CREATE_WARNINGS.AVATAR_IMPORT_FAILED);
    }

    return response;
}

export function registerCharacterRoutes(router, context, { jsonResponse, textResponse }) {
    router.post('/api/characters/all', async () => {
        const characters = await context.getAllCharacters({ shallow: true, forceRefresh: true });
        return jsonResponse(characters);
    });

    router.post('/api/characters/get', async ({ body }) => {
        let character;
        try {
            character = await context.getSingleCharacter(body);
        } catch (error) {
            if (isBadRequestError(error)) {
                return jsonResponse(badRequestBody(error), 400);
            }
            throw error;
        }

        if (!character) {
            return jsonResponse({ error: 'Character not found' }, 404);
        }

        return jsonResponse(character);
    });

    router.post('/api/characters/chats', async ({ body }) => {
        const avatar = pickAvatarIdentity(body);
        const simple = Boolean(body?.simple);
        const resolved = await resolveRouteCharacterId(context, { avatar, fallbackName: body?.ch_name || body?.name });
        if (resolved.responseBody) {
            return jsonResponse(resolved.responseBody, 400);
        }
        const characterId = resolved.characterId;

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
        if (body instanceof FormData) {
            const outcome = await context.createCharacterFromForm(body, url);
            await context.getAllCharacters({ shallow: true, forceRefresh: true });
            return createCharacterResponse(outcome, textResponse);
        }

        const outcome = await context.createCharacterFromPayload(body);
        await context.getAllCharacters({ shallow: true, forceRefresh: true });
        return createCharacterResponse(outcome, textResponse);
    });

    router.post('/api/characters/edit', async ({ body, url }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        await context.editCharacterFromForm(body, url);
        await context.getAllCharacters({ shallow: true, forceRefresh: true });
        return textResponse('ok');
    });

    router.post('/api/characters/lorebook-conflict', async ({ body }) => {
        const avatar = pickAvatarIdentity(body);
        const resolved = await resolveRouteCharacterId(context, { avatar, fallbackName: body?.name });
        if (resolved.responseBody) {
            return jsonResponse(resolved.responseBody, 400);
        }

        const characterId = resolved.characterId;
        if (!characterId) {
            return jsonResponse({ error: 'Character not found' }, 404);
        }

        const result = await context.safeInvoke('check_character_lorebook_conflict', {
            dto: { name: characterId },
        });

        return jsonResponse(result || { conflict: false });
    });

    router.post('/api/characters/resolve-lorebook-conflict', async ({ body }) => {
        const avatar = pickAvatarIdentity(body);
        const resolution = typeof body?.resolution === 'string' ? body.resolution : '';
        if (!['current', 'embedded'].includes(resolution)) {
            return jsonResponse({ error: 'Invalid lorebook conflict resolution' }, 400);
        }

        const resolved = await resolveRouteCharacterId(context, { avatar, fallbackName: body?.name });
        if (resolved.responseBody) {
            return jsonResponse(resolved.responseBody, 400);
        }

        const characterId = resolved.characterId;
        if (!characterId) {
            return jsonResponse({ error: 'Character not found' }, 404);
        }

        const result = await context.safeInvoke('resolve_character_lorebook_conflict', {
            dto: {
                name: characterId,
                resolution,
            },
        });

        await context.getAllCharacters({ shallow: true, forceRefresh: true });
        return jsonResponse(result || {});
    });

    router.post('/api/characters/edit-avatar', async ({ body, url }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        await context.editCharacterAvatarFromForm(body, url);
        return textResponse('OK');
    });

    router.post('/api/characters/delete', async ({ body }) => {
        const avatar = body?.avatar_url;
        const resolved = await resolveRouteCharacterId(context, { avatar, fallbackName: body?.name });
        if (resolved.responseBody) {
            return jsonResponse(resolved.responseBody, 400);
        }
        const characterId = resolved.characterId;

        if (!characterId) {
            return jsonResponse({ error: 'Character not found' }, 404);
        }

        await context.safeInvoke('delete_character', {
            dto: {
                name: characterId,
                delete_chats: Boolean(body?.delete_chats),
            },
        });

        await context.getAllCharacters({ shallow: true, forceRefresh: true });
        return jsonResponse({ ok: true });
    });

    router.post('/api/characters/rename', async ({ body }) => {
        const avatar = body?.avatar_url;
        const newName = body?.new_name || '';
        const resolved = await resolveRouteCharacterId(context, { avatar });
        if (resolved.responseBody) {
            return jsonResponse(resolved.responseBody, 400);
        }
        const oldCharacterId = resolved.characterId;

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
        await context.getAllCharacters({ shallow: true, forceRefresh: true });
        return jsonResponse(normalized);
    });

    router.post('/api/characters/duplicate', async ({ body }) => {
        let avatar = body?.avatar_url;
        if (!avatar) {
            return jsonResponse({ error: 'avatar URL not found' }, 400);
        }

        try {
            avatar = assertCharacterAvatarFileName(avatar, 'avatar_url', { required: true });
        } catch (error) {
            return jsonResponse(badRequestBody(error), 400);
        }

        const resolved = await resolveExistingRouteCharacterId(context, { avatar });
        if (resolved.responseBody) {
            return jsonResponse(resolved.responseBody, 400);
        }
        const originalCharacterId = resolved.characterId;
        if (!originalCharacterId) {
            return jsonResponse({ error: 'Character not found' }, 404);
        }

        const created = await context.safeInvoke('duplicate_character', {
            dto: { name: originalCharacterId },
        });
        const normalized = context.normalizeCharacter(created);
        await context.getAllCharacters({ shallow: true, forceRefresh: true });

        return jsonResponse({ path: normalized.avatar });
    });

    router.post('/api/characters/merge-attributes', async ({ body }) => {
        if (!body || typeof body !== 'object' || Array.isArray(body)) {
            return jsonResponse({ error: 'Expected JSON object body' }, 400);
        }

        if (Array.isArray(body.avatars)) {
            if (!body.data || typeof body.data !== 'object' || Array.isArray(body.data)) {
                return jsonResponse({ message: 'No valid update data provided.' }, 400);
            }

            let avatars = body.avatars;
            if (avatars.length > 0) {
                try {
                    avatars = avatars.map((avatar, index) =>
                        assertCharacterAvatarFileName(avatar, `avatars[${index}]`, { required: true }));
                } catch (error) {
                    return jsonResponse(badRequestBody(error), 400);
                }
            }

            const result = await context.safeInvoke('bulk_merge_character_card_data', {
                dto: {
                    avatars,
                    data: body.data,
                    filter: body.filter ?? null,
                },
            });
            await context.getAllCharacters({ shallow: true, forceRefresh: true });
            return jsonResponse(result);
        }

        let avatar = body?.avatar ?? body?.avatar_url;
        if (avatar !== undefined && avatar !== null) {
            try {
                const fieldName = Object.prototype.hasOwnProperty.call(body, 'avatar') ? 'avatar' : 'avatar_url';
                avatar = assertCharacterAvatarFileName(avatar, fieldName);
            } catch (error) {
                return jsonResponse(badRequestBody(error), 400);
            }
        }

        const update = buildCharacterMergeUpdate(
            avatar === undefined || avatar === null ? body : { ...body, avatar },
        );
        const resolved = await resolveRouteCharacterId(context, { avatar, fallbackName: body?.name });
        if (resolved.responseBody) {
            return jsonResponse(resolved.responseBody, 400);
        }
        const characterId = resolved.characterId;

        if (!characterId) {
            return jsonResponse({ error: 'Character not found' }, 404);
        }

        await context.safeInvoke('merge_character_card_data', {
            name: characterId,
            dto: {
                update,
            },
        });
        await context.getAllCharacters({ shallow: true, forceRefresh: true });

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
            kind: 'character-import',
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
        await context.getAllCharacters({ shallow: true, forceRefresh: true });
        const fileName = String(normalized.avatar || '').replace(/\.png$/i, '');

        return jsonResponse({
            file_name: fileName,
            character: normalized,
            post_import: {
                has_agent_profiles: hasCharacterEmbeddedAgentAsset(normalized, 'agentProfiles'),
                has_agent_skills: hasCharacterEmbeddedAgentAsset(normalized, 'skills'),
            },
        });
    });

    router.post('/api/characters/export', async ({ body }) => {
        const avatar = body?.avatar_url;
        const format = String(body?.format || 'json').toLowerCase();
        const resolved = await resolveRouteCharacterId(context, { avatar, fallbackName: body?.name });
        if (resolved.responseBody) {
            return jsonResponse(resolved.responseBody, 400);
        }
        const characterId = resolved.characterId;

        if (!characterId) {
            return jsonResponse({ error: 'Character not found' }, 404);
        }

        const normalizedFormat = format === 'json' ? 'json' : format === 'png' ? 'png' : '';
        if (!normalizedFormat) {
            return jsonResponse({ error: 'Unsupported export format' }, 400);
        }

        const exported = await context.safeInvoke('export_character_content', {
            dto: {
                name: characterId,
                format: normalizedFormat,
            },
        });

        const payload = normalizeBinaryPayload(exported?.data);
        if (normalizedFormat === 'png' && payload.byteLength === 0) {
            return jsonResponse({ error: 'Character export payload is empty' }, 500);
        }

        const contentType = String(
            exported?.mime_type || (normalizedFormat === 'png' ? 'image/png' : 'application/json'),
        );
        const fallbackName = `${characterId}.${normalizedFormat}`;
        const rawDownloadName = String(avatar || fallbackName).replace(/\.png$/i, `.${normalizedFormat}`);
        const downloadName = sanitizeAttachmentFileName(rawDownloadName, fallbackName);

        return new Response(payload, {
            status: 200,
            headers: {
                'Content-Type': contentType,
                'Content-Disposition': `attachment; filename="${encodeURI(downloadName)}"`,
            },
        });
    });
}
