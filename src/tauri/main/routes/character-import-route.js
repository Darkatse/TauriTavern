import { assertCharacterAvatarFileName } from '../services/characters/character-identity.js';
import { badRequestBody, resolveExistingRouteCharacterId } from './character-route-utils.js';

/**
 * @param {any} character
 * @param {'agentProfiles' | 'skills'} field
 */
function hasCharacterEmbeddedAgentAsset(character, field) {
    const sources = [
        character?.data?.extensions?.tauritavern,
        character?.extensions?.tauritavern,
    ];
    return sources.some(source => source && typeof source === 'object' && !Array.isArray(source)
        && source[field] !== undefined && source[field] !== null);
}

export function registerCharacterImportRoute(router, context, { jsonResponse }) {
    router.post('/api/characters/import', async ({ body }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        const file = body.get('avatar');
        if (!(file instanceof Blob)) {
            return jsonResponse({ error: 'No character file provided' }, 400);
        }

        let preserveFileName = null;
        let replacementName = null;
        const requestedPreservedName = body.get('preserved_name');
        if (requestedPreservedName !== null && String(requestedPreservedName).length > 0) {
            try {
                preserveFileName = assertCharacterAvatarFileName(requestedPreservedName, 'preserved_name', { required: true });
            } catch (error) {
                return jsonResponse(badRequestBody(error), 400);
            }

            const resolved = await resolveExistingRouteCharacterId(context, { avatar: preserveFileName });
            if (resolved.responseBody) {
                return jsonResponse(resolved.responseBody, 400);
            }
            replacementName = resolved.characterId;
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

        let imported;
        try {
            imported = replacementName
                ? await context.safeInvoke('replace_character', {
                    dto: { file_path: fileInfo.filePath, name: replacementName },
                })
                : await context.safeInvoke('import_character', {
                    dto: { file_path: fileInfo.filePath, preserve_file_name: preserveFileName },
                });
        } finally {
            await fileInfo.cleanup?.();
        }

        const normalized = context.normalizeCharacter(imported);
        const fileName = String(normalized.avatar || '').replace(/\.png$/i, '');

        return jsonResponse({
            file_name: fileName,
            replaced: Boolean(replacementName),
            character: normalized,
            post_import: {
                has_agent_profiles: hasCharacterEmbeddedAgentAsset(normalized, 'agentProfiles'),
                has_agent_skills: hasCharacterEmbeddedAgentAsset(normalized, 'skills'),
            },
        });
    });
}
