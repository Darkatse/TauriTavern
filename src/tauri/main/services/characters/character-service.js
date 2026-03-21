// @ts-check

/**
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any) => Promise<any>} SafeInvokeFn
 */

/**
 * @param {{ safeInvoke: SafeInvokeFn }} deps
 */
export function createCharacterService({ safeInvoke }) {
    /** @type {any[]} */
    let characterCache = [];
    /** @type {Map<string, any>} */
    let characterByAvatar = new Map();
    /** @type {Map<string, any>} */
    let characterByDisplayName = new Map();
    /** @type {Map<string, any>} */
    let characterById = new Map();

    /** @param {any} input */
    function normalizeExtensions(input) {
        if (!input || typeof input !== 'object' || Array.isArray(input)) {
            return {};
        }

        return { ...input };
    }

    /** @param {...any} values */
    function pickCharacterTextValue(...values) {
        for (const value of values) {
            if (typeof value === 'string' && value.length > 0) {
                return value;
            }
        }

        return '';
    }

    /** @param {any} character */
    function normalizeCharacter(character) {
        if (!character || typeof character !== 'object') {
            return character;
        }

        const extensions = normalizeExtensions(character.extensions);

        if (!Object.prototype.hasOwnProperty.call(extensions, 'talkativeness')) {
            extensions.talkativeness = Number(character.talkativeness ?? 0.5);
        }

        if (!Object.prototype.hasOwnProperty.call(extensions, 'fav')) {
            extensions.fav = Boolean(character.fav);
        }

        const characterBook = Object.prototype.hasOwnProperty.call(character, 'character_book')
            ? character.character_book
            : character?.data?.character_book;

        const name = pickCharacterTextValue(character.name, character?.data?.name);
        const description = pickCharacterTextValue(character.description, character?.data?.description);
        const personality = pickCharacterTextValue(character.personality, character?.data?.personality);
        const scenario = pickCharacterTextValue(character.scenario, character?.data?.scenario);
        const firstMes = pickCharacterTextValue(character.first_mes, character?.data?.first_mes);
        const mesExample = pickCharacterTextValue(character.mes_example, character?.data?.mes_example);
        const creator = pickCharacterTextValue(character.creator, character?.data?.creator);
        const creatorNotes = pickCharacterTextValue(character.creator_notes, character?.data?.creator_notes);
        const characterVersion = pickCharacterTextValue(character.character_version, character?.data?.character_version);
        const systemPrompt = pickCharacterTextValue(character.system_prompt, character?.data?.system_prompt);
        const postHistoryInstructions = pickCharacterTextValue(
            character.post_history_instructions,
            character?.data?.post_history_instructions,
        );

        const data = {
            name,
            description,
            personality,
            scenario,
            first_mes: firstMes,
            mes_example: mesExample,
            creator,
            creator_notes: creatorNotes,
            character_version: characterVersion,
            system_prompt: systemPrompt,
            post_history_instructions: postHistoryInstructions,
            tags: Array.isArray(character.tags) ? character.tags : [],
            alternate_greetings: Array.isArray(character.alternate_greetings) ? character.alternate_greetings : [],
            character_book: characterBook ?? null,
            extensions,
        };

        return {
            ...character,
            name,
            description,
            personality,
            scenario,
            first_mes: firstMes,
            mes_example: mesExample,
            creator,
            creator_notes: creatorNotes,
            character_version: characterVersion,
            system_prompt: systemPrompt,
            post_history_instructions: postHistoryInstructions,
            creatorcomment: creatorNotes,
            data,
            shallow: Boolean(character.shallow),
        };
    }

    /** @param {any} avatar */
    function normalizeAvatarFileName(avatar) {
        if (avatar === null || avatar === undefined) {
            return null;
        }

        let value = String(avatar).trim();
        if (!value) {
            return null;
        }

        if (value.includes('?')) {
            try {
                const parsed = new URL(value, 'http://localhost');
                value = parsed.searchParams.get('file') || parsed.pathname || value;
            } catch {
                // Keep original value when URL parsing fails.
            }
        }

        try {
            value = decodeURIComponent(value);
        } catch {
            // Keep original value when decodeURIComponent fails.
        }

        value = value.replace(/[?#].*$/, '');
        if (!value) {
            return null;
        }

        const normalized = value.replace(/[\\/]+/g, '/');
        const fileName = normalized.split('/').pop();
        return fileName || null;
    }

    /** @param {any} avatar */
    function getAvatarInternalId(avatar) {
        const fileName = normalizeAvatarFileName(avatar);
        if (!fileName) {
            return null;
        }

        return fileName.replace(/\.[^/.]+$/, '') || null;
    }

    /** @param {any} character */
    function getCharacterId(character) {
        if (!character || typeof character !== 'object') {
            return null;
        }

        const fromAvatar = getAvatarInternalId(character.avatar);
        if (fromAvatar) {
            return fromAvatar;
        }

        if (character.name) {
            return String(character.name);
        }

        return null;
    }

    /** @param {any} characters */
    function updateCharacterCache(characters) {
        characterCache = Array.isArray(characters) ? characters : [];
        characterByAvatar = new Map();
        characterByDisplayName = new Map();
        characterById = new Map();

        for (const character of characterCache) {
            if (character?.avatar) {
                const rawAvatar = String(character.avatar);
                characterByAvatar.set(rawAvatar, character);

                const normalizedAvatar = normalizeAvatarFileName(rawAvatar);
                if (normalizedAvatar) {
                    characterByAvatar.set(normalizedAvatar, character);
                }
            }

            if (character?.name) {
                characterByDisplayName.set(String(character.name), character);
            }

            const characterId = getCharacterId(character);
            if (characterId) {
                characterById.set(characterId, character);
            }
        }
    }

    /** @param {boolean} requestShallow */
    function canReuseCharacterCache(requestShallow) {
        if (characterCache.length === 0) {
            return false;
        }

        if (requestShallow) {
            return true;
        }

        return characterCache.every((character) => !Boolean(character?.shallow));
    }

    /**
     * @param {{ shallow?: boolean; forceRefresh?: boolean } | undefined} options
     */
    async function getAllCharacters(options = {}) {
        const shallow = options.shallow ?? true;
        const forceRefresh = options.forceRefresh ?? false;
        if (!forceRefresh && canReuseCharacterCache(shallow)) {
            return characterCache;
        }

        const characters = await safeInvoke('get_all_characters', { shallow });
        const normalized = Array.isArray(characters) ? characters.map(normalizeCharacter) : [];
        updateCharacterCache(normalized);
        return normalized;
    }

    /**
     * @param {{ avatar?: any; fallbackName?: string } | undefined} options
     */
    async function resolveCharacterId(options = {}) {
        const avatar = options.avatar;
        const fallbackName = options.fallbackName;
        const avatarInternalId = getAvatarInternalId(avatar);
        const avatarFileName = normalizeAvatarFileName(avatar);

        const resolveFromCache = () => {
            if (avatar !== undefined && avatar !== null) {
                const fromRawAvatar = characterByAvatar.get(String(avatar));
                const fromRawAvatarId = getCharacterId(fromRawAvatar);
                if (fromRawAvatarId) {
                    return fromRawAvatarId;
                }
            }

            if (avatarFileName) {
                const fromFileName = characterByAvatar.get(avatarFileName);
                const fromFileNameId = getCharacterId(fromFileName);
                if (fromFileNameId) {
                    return fromFileNameId;
                }
            }

            if (avatarInternalId) {
                const fromInternalId = characterById.get(avatarInternalId);
                const fromInternalIdValue = getCharacterId(fromInternalId);
                if (fromInternalIdValue) {
                    return fromInternalIdValue;
                }
            }

            return null;
        };

        if (avatarInternalId || avatarFileName) {
            const cached = resolveFromCache();
            if (cached) {
                return cached;
            }

            await getAllCharacters({ shallow: true });
            const refreshed = resolveFromCache();
            if (refreshed) {
                return refreshed;
            }
        }

        const fallback = String(fallbackName || '').trim();
        if (!fallback) {
            return avatarInternalId || null;
        }

        const cachedByName = characterByDisplayName.get(fallback);
        const cachedByNameId = getCharacterId(cachedByName);
        if (cachedByNameId) {
            return cachedByNameId;
        }

        const cachedByInternalId = characterById.get(fallback);
        const cachedByInternalIdValue = getCharacterId(cachedByInternalId);
        if (cachedByInternalIdValue) {
            return cachedByInternalIdValue;
        }

        await getAllCharacters({ shallow: true });
        const refreshedByName = characterByDisplayName.get(fallback);
        const refreshedByNameId = getCharacterId(refreshedByName);
        if (refreshedByNameId) {
            return refreshedByNameId;
        }

        const refreshedByInternalId = characterById.get(fallback);
        const refreshedByInternalIdValue = getCharacterId(refreshedByInternalId);
        if (refreshedByInternalIdValue) {
            return refreshedByInternalIdValue;
        }

        return avatarInternalId || fallback;
    }

    /** @param {any} body */
    async function getSingleCharacter(body) {
        const explicitName = body?.name || body?.ch_name;
        const avatar = body?.avatar_url || body?.avatar;
        const characterId = await resolveCharacterId({ avatar, fallbackName: explicitName });

        if (!characterId) {
            return null;
        }

        const character = await safeInvoke('get_character', { name: characterId });
        const normalized = normalizeCharacter(character);
        const normalizedAvatar = normalized?.avatar ? String(normalized.avatar) : '';
        if (normalizedAvatar) {
            const index = characterCache.findIndex((item) => String(item?.avatar || '') === normalizedAvatar);
            if (index >= 0) {
                characterCache[index] = normalized;
            }
        }
        if (normalized?.avatar) {
            characterByAvatar.set(String(normalized.avatar), normalized);
        }
        if (normalized?.name) {
            characterByDisplayName.set(String(normalized.name), normalized);
        }
        const normalizedCharacterId = getCharacterId(normalized);
        if (normalizedCharacterId) {
            characterById.set(normalizedCharacterId, normalized);
        }
        return normalized;
    }

    /** @param {any} characterId */
    function findAvatarByCharacterId(characterId) {
        const key = String(characterId || '').trim();
        if (!key) {
            return '';
        }

        const byDisplayName = characterByDisplayName.get(key);
        if (byDisplayName?.avatar) {
            return byDisplayName.avatar;
        }

        const byInternalId = characterById.get(key);
        if (byInternalId?.avatar) {
            return byInternalId.avatar;
        }

        const normalizedAvatar = normalizeAvatarFileName(key);
        if (normalizedAvatar) {
            const byAvatar = characterByAvatar.get(normalizedAvatar);
            if (byAvatar?.avatar) {
                return byAvatar.avatar;
            }

            if (normalizedAvatar.toLowerCase().endsWith('.png')) {
                return normalizedAvatar;
            }

            const pngName = `${normalizedAvatar}.png`;
            const byPng = characterByAvatar.get(pngName);
            if (byPng?.avatar) {
                return byPng.avatar;
            }

            return pngName;
        }

        return '';
    }

    /** @param {string} baseName */
    async function uniqueCharacterName(baseName) {
        await getAllCharacters({ shallow: true });

        if (!characterByDisplayName.has(baseName)) {
            return baseName;
        }

        let index = 2;
        while (characterByDisplayName.has(`${baseName} ${index}`)) {
            index += 1;
        }

        return `${baseName} ${index}`;
    }

    return {
        normalizeCharacter,
        normalizeExtensions,
        getAllCharacters,
        resolveCharacterId,
        getSingleCharacter,
        findAvatarByCharacterId,
        uniqueCharacterName,
    };
}
