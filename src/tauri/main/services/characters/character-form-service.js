// @ts-check

/**
 * @typedef {import('../../context/types.js').MaterializedFileInfo} MaterializedFileInfo
 */

/**
 * @typedef {(command: string, args?: any) => Promise<any>} SafeInvokeFn
 * @typedef {(command: string) => void} InvalidateInvokeAllFn
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

    /** @param {any} value @param {any} [fallback] */
    function parseJsonSafe(value, fallback = {}) {
        if (typeof value !== 'string' || !value.trim()) {
            return fallback;
        }

        try {
            return JSON.parse(value);
        } catch {
            return fallback;
        }
    }

    /** @param {FormData} formData */
    function buildCharacterExtensions(formData) {
        const extensions = parseJsonSafe(stringFromForm(formData, 'extensions', ''), {});

        const world = stringFromForm(formData, 'world', '').trim();
        if (world) {
            extensions.world = world;
        }

        const depthPrompt = stringFromForm(formData, 'depth_prompt_prompt', '').trim();
        if (depthPrompt) {
            extensions.depth_prompt = {
                prompt: depthPrompt,
                depth: numberFromForm(formData, 'depth_prompt_depth', 4),
                role: stringFromForm(formData, 'depth_prompt_role', 'system'),
            };
        }

        extensions.talkativeness = numberFromForm(formData, 'talkativeness', 0.5);
        extensions.fav = boolFromForm(formData, 'fav');

        return extensions;
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

    /** @param {FormData} formData */
    function formDataToUpdateCharacterDto(formData) {
        const dto = formDataToCreateCharacterDto(formData);
        const chat = stringFromForm(formData, 'chat', '').trim();

        return {
            name: dto.name,
            chat: chat || undefined,
            description: dto.description,
            personality: dto.personality,
            scenario: dto.scenario,
            first_mes: dto.first_mes,
            mes_example: dto.mes_example,
            creator: dto.creator,
            creator_notes: dto.creator_notes,
            character_version: dto.character_version,
            tags: dto.tags,
            talkativeness: dto.talkativeness,
            fav: dto.fav,
            alternate_greetings: dto.alternate_greetings,
            system_prompt: dto.system_prompt,
            post_history_instructions: dto.post_history_instructions,
            extensions: dto.extensions,
        };
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

    /** @param {any} payload */
    function pickCharacterUpdateFields(payload) {
        /** @type {Record<string, any>} */
        const dto = {};
        const keys = [
            'name',
            'chat',
            'description',
            'personality',
            'scenario',
            'first_mes',
            'mes_example',
            'creator',
            'creator_notes',
            'character_version',
            'tags',
            'talkativeness',
            'fav',
            'alternate_greetings',
            'system_prompt',
            'post_history_instructions',
            'extensions',
        ];

        for (const key of keys) {
            if (Object.prototype.hasOwnProperty.call(payload, key)) {
                // @ts-ignore - dynamic indexing by payload schema.
                dto[key] = payload[key];
            }
        }

        const data = payload?.data;
        if (data && typeof data === 'object') {
            for (const key of keys) {
                if (key !== 'extensions' && !Object.prototype.hasOwnProperty.call(dto, key) && Object.prototype.hasOwnProperty.call(data, key)) {
                    // @ts-ignore - dynamic indexing by payload schema.
                    dto[key] = data[key];
                }
            }

            if (Object.prototype.hasOwnProperty.call(data, 'extensions')) {
                dto.extensions = Object.prototype.hasOwnProperty.call(dto, 'extensions')
                    ? { ...dto.extensions, ...data.extensions }
                    : data.extensions;
            }
        }

        return dto;
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

        const dto = formDataToUpdateCharacterDto(formData);
        await safeInvoke('update_character', { name: originalCharacterId, dto });

        const file = formData.get('avatar');
        if (file instanceof Blob && file.size > 0) {
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
                await safeInvoke('update_avatar', {
                    dto: {
                        name: originalCharacterId,
                        avatar_path: fileInfo.filePath,
                        crop,
                    },
                });

                invalidateInvokeAll('read_thumbnail_asset');
            } finally {
                await fileInfo.cleanup?.();
            }
        }
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
        pickCharacterUpdateFields,
        createCharacterFromForm,
        editCharacterFromForm,
        uploadAvatarFromForm,
    };
}
