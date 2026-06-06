// @ts-check

import { getByPath, mergeObjects, setByPath, unsetByPath } from './form-object-utils.js';
import { formDataToCreateCharacterDto, parseJsonObjectStrict } from './character-create-mapper.js';
import { assertCharacterAvatarFileName } from './character-identity.js';
import { parseCropParam } from './character-request-utils.js';

/**
 * @typedef {import('../../context/types.js').MaterializedFileInfo} MaterializedFileInfo
 */

/**
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any) => Promise<any>} SafeInvokeFn
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand) => void} InvalidateInvokeAllFn
 * @typedef {(options?: { avatar?: any; fallbackName?: string }) => Promise<string | null>} ResolveCharacterIdFn
 * @typedef {(options?: { avatar?: any; fallbackName?: string }) => Promise<string | null>} ResolveExistingCharacterIdFn
 * @typedef {(file: Blob, options?: { preferredName?: string; preferredExtension?: string }) => Promise<MaterializedFileInfo | null>} MaterializeUploadFileFn
 */

/**
 * @param {{
 *   safeInvoke: SafeInvokeFn;
 *   invalidateInvokeAll: InvalidateInvokeAllFn;
 *   resolveCharacterId: ResolveCharacterIdFn;
 *   resolveExistingCharacterId: ResolveExistingCharacterIdFn;
 *   materializeUploadFile: MaterializeUploadFileFn;
 * }} deps
 */
export function createCharacterFormService({
    safeInvoke,
    invalidateInvokeAll,
    resolveCharacterId,
    resolveExistingCharacterId,
    materializeUploadFile,
}) {
    /** @param {FormData} formData @param {string} key @param {string} [fallback] */
    function stringFromForm(formData, key, fallback = '') {
        const raw = formData.get(key);
        if (raw === null || raw === undefined) {
            return fallback;
        }

        return String(raw);
    }

    /** @param {FormData} formData */
    function avatarUrlFromForm(formData) {
        return assertCharacterAvatarFileName(formData.get('avatar_url'), 'avatar_url', { required: true });
    }

    /**
     * @param {Record<string, any>} target
     * @param {Record<string, any>} values
     */
    function assignObjectPaths(target, values) {
        for (const [path, value] of Object.entries(values)) {
            setByPath(target, path, value);
        }
    }

    /** @param {FormData} formData */
    function buildCharacterCardFromForm(formData) {
        const dto = formDataToCreateCharacterDto(formData);
        const baseCard = parseJsonObjectStrict(stringFromForm(formData, 'json_data', ''), {}, "character json_data");
        const name = dto.name.trim();

        if (!name) {
            throw new Error('Character name is required');
        }

        const chat = formData.has('chat')
            ? stringFromForm(formData, 'chat', '').trim()
            : `${name} - ${new Date().toISOString()}`;
        const createDate = stringFromForm(formData, 'create_date', '').trim();
        const mergedExtensions = mergeObjects({}, getByPath(baseCard, 'data.extensions', {}), dto.extensions);

        unsetByPath(baseCard, 'json_data');

        assignObjectPaths(baseCard, {
            name,
            description: dto.description,
            personality: dto.personality,
            scenario: dto.scenario,
            first_mes: dto.first_mes,
            mes_example: dto.mes_example,
            creatorcomment: dto.creator_notes,
            avatar: 'none',
            talkativeness: dto.talkativeness,
            fav: dto.fav,
            tags: dto.tags,
            spec: 'chara_card_v2',
            spec_version: '2.0',
            'data.name': name,
            'data.description': dto.description,
            'data.personality': dto.personality,
            'data.scenario': dto.scenario,
            'data.first_mes': dto.first_mes,
            'data.mes_example': dto.mes_example,
            'data.creator_notes': dto.creator_notes,
            'data.system_prompt': dto.system_prompt,
            'data.post_history_instructions': dto.post_history_instructions,
            'data.tags': dto.tags,
            'data.creator': dto.creator,
            'data.character_version': dto.character_version,
            'data.alternate_greetings': dto.alternate_greetings,
            'data.extensions': mergedExtensions,
        });

        if (typeof mergedExtensions.world === 'string' && mergedExtensions.world.trim()) {
            unsetByPath(baseCard, 'data.character_book');
        }

        if (formData.has('chat')) {
            if (chat) {
                setByPath(baseCard, 'chat', chat);
            } else {
                unsetByPath(baseCard, 'chat');
            }
        } else {
            setByPath(baseCard, 'chat', chat);
        }

        if (formData.has('create_date')) {
            if (createDate) {
                setByPath(baseCard, 'create_date', createDate);
            } else {
                unsetByPath(baseCard, 'create_date');
            }
        }

        return baseCard;
    }

    /** @param {FormData} formData @param {URL} requestUrl */
    async function editCharacterFromForm(formData, requestUrl) {
        const avatar = avatarUrlFromForm(formData);
        const originalCharacterId = await resolveCharacterId({ avatar });

        if (!originalCharacterId) {
            throw new Error('Character not found for edit');
        }

        const file = formData.get('avatar');
        const crop = parseCropParam(requestUrl);
        const card = buildCharacterCardFromForm(formData);

        if (file instanceof Blob && file.size > 0) {
            const preferredName = file instanceof File ? file.name : '';
            const fileInfo = await materializeUploadFile(file, {
                preferredName,
            });

            if (!fileInfo?.filePath) {
                const reason = fileInfo?.error ? `: ${fileInfo.error}` : '';
                throw new Error(`Bad request: unable to access avatar file path${reason}`);
            }

            try {
                await safeInvoke('update_character_card_data', {
                    name: originalCharacterId,
                    dto: {
                        card_json: JSON.stringify(card),
                        avatar_path: fileInfo.filePath,
                        crop: crop || null,
                    },
                });

                invalidateInvokeAll('read_thumbnail_asset');
            } finally {
                await fileInfo.cleanup?.();
            }

            return;
        }

        await safeInvoke('update_character_card_data', {
            name: originalCharacterId,
            dto: {
                card_json: JSON.stringify(card),
                avatar_path: null,
                crop: crop || null,
            },
        });
    }

    /** @param {FormData} formData @param {URL} requestUrl */
    async function editCharacterAvatarFromForm(formData, requestUrl) {
        const avatar = avatarUrlFromForm(formData);
        const file = formData.get('avatar');
        if (!(file instanceof Blob) || file.size === 0) {
            throw new Error('Bad request: no file uploaded');
        }

        const characterId = await resolveExistingCharacterId({ avatar });
        if (!characterId) {
            throw new Error('Bad request: character file does not exist');
        }

        const crop = parseCropParam(requestUrl);
        const preferredName = file instanceof File ? file.name : '';
        const fileInfo = await materializeUploadFile(file, {
            preferredName,
        });
        if (!fileInfo?.filePath) {
            const reason = fileInfo?.error ? `: ${fileInfo.error}` : '';
            throw new Error(`Bad request: unable to access uploaded avatar file${reason}`);
        }

        try {
            await safeInvoke('update_avatar', {
                dto: {
                    name: characterId,
                    avatar_path: fileInfo.filePath,
                    crop: crop || null,
                },
            });
            invalidateInvokeAll('read_thumbnail_asset');
        } finally {
            await fileInfo.cleanup?.();
        }
    }

    /** @param {FormData} formData @param {URL} requestUrl */
    async function uploadAvatarFromForm(formData, requestUrl) {
        const file = formData.get('avatar');
        if (!(file instanceof Blob)) {
            throw new Error('Bad request: no avatar file provided');
        }

        const overwriteNameRaw = formData.get('overwrite_name');
        const overwriteName = overwriteNameRaw ? String(overwriteNameRaw) : null;
        const crop = parseCropParam(requestUrl);

        const preferredName = file instanceof File ? file.name : '';
        const fileInfo = await materializeUploadFile(file, {
            preferredName,
        });
        if (!fileInfo?.filePath) {
            const reason = fileInfo?.error ? `: ${fileInfo.error}` : '';
            throw new Error(`Bad request: unable to access avatar file path${reason}`);
        }

        try {
            const uploaded = await safeInvoke('upload_avatar', {
                file_path: fileInfo.filePath,
                overwrite_name: overwriteName,
                crop: crop ? JSON.stringify(crop) : null,
            });
            invalidateInvokeAll('read_thumbnail_asset');
            return uploaded;
        } finally {
            await fileInfo.cleanup?.();
        }
    }

    return {
        editCharacterFromForm,
        editCharacterAvatarFromForm,
        uploadAvatarFromForm,
    };
}
