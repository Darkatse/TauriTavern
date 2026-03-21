// @ts-check

import { lodash as _ } from '../../../../lib.js';

/**
 * @typedef {import('../../context/types.js').MaterializedFileInfo} MaterializedFileInfo
 */

/**
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any) => Promise<any>} SafeInvokeFn
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand) => void} InvalidateInvokeAllFn
 * @typedef {(options?: { avatar?: any; fallbackName?: string }) => Promise<string | null>} ResolveCharacterIdFn
 * @typedef {(file: Blob, options?: { preferredName?: string; preferredExtension?: string }) => Promise<MaterializedFileInfo | null>} MaterializeUploadFileFn
 */

/**
 * @param {{
 *   safeInvoke: SafeInvokeFn;
 *   invalidateInvokeAll: InvalidateInvokeAllFn;
 *   resolveCharacterId: ResolveCharacterIdFn;
 *   materializeUploadFile: MaterializeUploadFileFn;
 * }} deps
 */
export function createCharacterFormService({
    safeInvoke,
    invalidateInvokeAll,
    resolveCharacterId,
    materializeUploadFile,
}) {
    /** @param {FormData} formData @param {string} key */
    function boolFromForm(formData, key) {
        const raw = formData.get(key);
        if (raw === null || raw === undefined) {
            return false;
        }

        const value = String(raw).trim().toLowerCase();
        return value === 'true' || value === '1' || value === 'on' || value === 'yes';
    }

    /** @param {FormData} formData @param {string} key @param {number} fallback */
    function numberFromForm(formData, key, fallback) {
        const raw = formData.get(key);
        const value = Number(raw);
        return Number.isFinite(value) ? value : fallback;
    }

    /** @param {FormData} formData @param {string} key @param {string} [fallback] */
    function stringFromForm(formData, key, fallback = '') {
        const raw = formData.get(key);
        if (raw === null || raw === undefined) {
            return fallback;
        }

        return String(raw);
    }

    /** @param {FormData} formData @param {string} key */
    function arrayNotationValuesFromForm(formData, key) {
        const values = [];

        for (const [entryKey, entryValue] of formData.entries()) {
            if (entryKey === `${key}[]` || (entryKey.startsWith(`${key}[`) && entryKey.endsWith(']'))) {
                const value = String(entryValue);
                if (value) {
                    values.push(value);
                }
            }
        }

        return values;
    }

    /** @param {any} tagsRaw */
    function splitTags(tagsRaw) {
        if (Array.isArray(tagsRaw)) {
            return tagsRaw.map((tag) => String(tag).trim()).filter(Boolean);
        }

        if (typeof tagsRaw === 'string') {
            return tagsRaw.split(',').map((tag) => tag.trim()).filter(Boolean);
        }

        return [];
    }

    /** @param {any} value @param {any} [fallback] @param {string} [label] */
    function parseJsonStrict(value, fallback = {}, label = 'JSON payload') {
        if (typeof value !== 'string' || !value.trim()) {
            return fallback;
        }

        try {
            const parsed = JSON.parse(value);
            if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
                throw new Error('Expected JSON object');
            }
            return parsed;
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            throw new Error(`Invalid ${label}: ${message}`);
        }
    }

    /** @param {FormData} formData */
    function buildCharacterExtensions(formData) {
        const defaults = {
            world: stringFromForm(formData, 'world', '').trim(),
            depth_prompt: {
                prompt: stringFromForm(formData, 'depth_prompt_prompt', ''),
                depth: numberFromForm(formData, 'depth_prompt_depth', 4),
                role: stringFromForm(formData, 'depth_prompt_role', 'system'),
            },
            talkativeness: numberFromForm(formData, 'talkativeness', 0.5),
            fav: boolFromForm(formData, 'fav'),
        };
        const parsed = parseJsonStrict(stringFromForm(formData, 'extensions', ''), {}, "extensions JSON");

        return _.merge({}, defaults, parsed);
    }

    /** @param {FormData} formData */
    function formDataToCreateCharacterDto(formData) {
        const alternateGreetings = formData.getAll('alternate_greetings').map((item) => String(item)).filter(Boolean);
        const bracketAlternateGreetings = arrayNotationValuesFromForm(formData, 'alternate_greetings');
        const bracketTags = arrayNotationValuesFromForm(formData, 'tags');

        return {
            name: stringFromForm(formData, 'ch_name', '').trim(),
            description: stringFromForm(formData, 'description', ''),
            personality: stringFromForm(formData, 'personality', ''),
            scenario: stringFromForm(formData, 'scenario', ''),
            first_mes: stringFromForm(formData, 'first_mes', ''),
            mes_example: stringFromForm(formData, 'mes_example', ''),
            creator: stringFromForm(formData, 'creator', ''),
            creator_notes: stringFromForm(formData, 'creator_notes', ''),
            character_version: stringFromForm(formData, 'character_version', ''),
            tags: bracketTags.length > 0 ? splitTags(bracketTags) : splitTags(stringFromForm(formData, 'tags', '')),
            talkativeness: numberFromForm(formData, 'talkativeness', 0.5),
            fav: boolFromForm(formData, 'fav'),
            alternate_greetings: alternateGreetings.length > 0 ? alternateGreetings : bracketAlternateGreetings,
            system_prompt: stringFromForm(formData, 'system_prompt', ''),
            post_history_instructions: stringFromForm(formData, 'post_history_instructions', ''),
            extensions: buildCharacterExtensions(formData),
        };
    }

    /**
     * @param {Record<string, any>} target
     * @param {Record<string, any>} values
     */
    function assignObjectPaths(target, values) {
        for (const [path, value] of Object.entries(values)) {
            _.set(target, path, value);
        }
    }

    /** @param {FormData} formData */
    function buildCharacterCardFromForm(formData) {
        const dto = formDataToCreateCharacterDto(formData);
        const baseCard = parseJsonStrict(stringFromForm(formData, 'json_data', ''), {}, "character json_data");
        const name = dto.name.trim();

        if (!name) {
            throw new Error('Character name is required');
        }

        const chat = formData.has('chat')
            ? stringFromForm(formData, 'chat', '').trim()
            : `${name} - ${new Date().toISOString()}`;
        const createDate = stringFromForm(formData, 'create_date', '').trim();
        const mergedExtensions = _.merge({}, _.get(baseCard, 'data.extensions', {}), dto.extensions);

        _.unset(baseCard, 'json_data');

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
            _.unset(baseCard, 'data.character_book');
        }

        if (formData.has('chat')) {
            if (chat) {
                _.set(baseCard, 'chat', chat);
            } else {
                _.unset(baseCard, 'chat');
            }
        } else {
            _.set(baseCard, 'chat', chat);
        }

        if (formData.has('create_date')) {
            if (createDate) {
                _.set(baseCard, 'create_date', createDate);
            } else {
                _.unset(baseCard, 'create_date');
            }
        }

        return baseCard;
    }

    /** @param {any} value */
    function toRoundedInt(value) {
        const number = Number(value);
        if (!Number.isFinite(number)) {
            return null;
        }

        return Math.round(number);
    }

    /**
     * @param {URL} url
     */
    function parseCropParam(url) {
        const raw = url.searchParams.get('crop');
        if (!raw) {
            return null;
        }

        try {
            const crop = JSON.parse(raw);
            if (!crop || typeof crop !== 'object') {
                return null;
            }

            const x = toRoundedInt(crop.x);
            const y = toRoundedInt(crop.y);
            const width = toRoundedInt(crop.width);
            const height = toRoundedInt(crop.height);
            if (x === null || y === null || width === null || height === null) {
                return null;
            }

            return {
                x,
                y,
                width,
                height,
                want_resize: Boolean(crop.want_resize),
            };
        } catch {
            return null;
        }
    }

    /** @param {FormData} formData @param {URL} requestUrl */
    async function createCharacterFromForm(formData, requestUrl) {
        const dto = formDataToCreateCharacterDto(formData);
        const crop = parseCropParam(requestUrl);
        const file = formData.get('avatar');

        if (file instanceof Blob && file.size > 0) {
            const preferredName = file instanceof File ? file.name : '';
            const fileInfo = await materializeUploadFile(file, {
                preferredName,
            });
            if (!fileInfo?.filePath) {
                const reason = fileInfo?.error ? `: ${fileInfo.error}` : '';
                throw new Error(`Unable to access avatar file path${reason}`);
            }

            try {
                return await safeInvoke('create_character_with_avatar', {
                    dto: {
                        character: dto,
                        avatar_path: fileInfo.filePath,
                        crop,
                    },
                });
            } finally {
                await fileInfo.cleanup?.();
            }
        }

        return safeInvoke('create_character', { dto });
    }

    /** @param {FormData} formData @param {URL} requestUrl */
    async function editCharacterFromForm(formData, requestUrl) {
        const avatar = stringFromForm(formData, 'avatar_url', '');
        const fallbackName = stringFromForm(formData, 'ch_name', '');
        const originalCharacterId = await resolveCharacterId({ avatar, fallbackName });

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
                throw new Error(`Unable to access avatar file path${reason}`);
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
    async function uploadAvatarFromForm(formData, requestUrl) {
        const file = formData.get('avatar');
        if (!(file instanceof Blob)) {
            throw new Error('No avatar file provided');
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
            throw new Error(`Unable to access avatar file path${reason}`);
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
        createCharacterFromForm,
        editCharacterFromForm,
        uploadAvatarFromForm,
    };
}
