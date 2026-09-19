// @ts-check

import {
    assertCharacterAvatarFileName,
    characterStemFromAvatarFileName,
    hasCharacterAvatarIdentity,
} from './character-identity.js';

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
    function rawCharacterFromJsonData(character) {
        if (typeof character?.json_data !== 'string' || !character.json_data.trim()) {
            return {};
        }

        const value = JSON.parse(character.json_data);
        if (!value || typeof value !== 'object' || Array.isArray(value)) {
            throw new Error('Backend returned non-object character json_data');
        }

        return value;
    }

    /** @param {any} character */
    function normalizeCharacter(character) {
        if (!character || typeof character !== 'object') {
            return character;
        }

        const rawCharacter = rawCharacterFromJsonData(character);
        if (hasBodyField(rawCharacter, 'spec')) {
            const normalized = { ...rawCharacter };
            const rawData = rawCharacter.data;

            if (rawData !== undefined) {
                const fieldMappings = {
                    name: rawData?.name,
                    description: rawData?.description,
                    personality: rawData?.personality,
                    scenario: rawData?.scenario,
                    first_mes: rawData?.first_mes,
                    mes_example: rawData?.mes_example,
                    talkativeness: rawData?.extensions?.talkativeness,
                    fav: rawData?.extensions?.fav,
                    tags: rawData?.tags,
                };

                for (const [field, value] of Object.entries(fieldMappings)) {
                    if (value !== undefined) {
                        normalized[field] = value;
                    } else if (field === 'talkativeness') {
                        normalized[field] = 0.5;
                    } else if (field === 'fav') {
                        normalized[field] = false;
                    }
                }
            }

            normalized.chat ??= character.chat;
            if (!normalized.create_date && hasBodyField(character, 'create_date')) {
                normalized.create_date = character.create_date;
            }
            for (const field of [
                'avatar',
                'chat_size',
                'data_size',
                'date_added',
                'date_last_chat',
                'json_data',
                'shallow',
            ]) {
                if (hasBodyField(character, field)) {
                    normalized[field] = character[field];
                }
            }

            return normalized;
        }

        const rawData = normalizeExtensions(rawCharacter.data);
        const projectedData = normalizeExtensions(character.data);
        const extensions = normalizeExtensions(character.extensions);

        if (!Object.prototype.hasOwnProperty.call(extensions, 'talkativeness')) {
            extensions.talkativeness = Number(character.talkativeness ?? 0.5);
        }

        if (!Object.prototype.hasOwnProperty.call(extensions, 'fav')) {
            extensions.fav = Boolean(character.fav);
        }

        const characterBook = Object.prototype.hasOwnProperty.call(character, 'character_book')
            ? character.character_book
            : projectedData.character_book ?? rawData.character_book;

        const name = pickCharacterTextValue(character.name, projectedData.name, rawData.name, rawCharacter.name);
        const description = pickCharacterTextValue(character.description, projectedData.description, rawData.description, rawCharacter.description);
        const personality = pickCharacterTextValue(character.personality, projectedData.personality, rawData.personality, rawCharacter.personality);
        const scenario = pickCharacterTextValue(character.scenario, projectedData.scenario, rawData.scenario, rawCharacter.scenario);
        const firstMes = pickCharacterTextValue(character.first_mes, projectedData.first_mes, rawData.first_mes, rawCharacter.first_mes);
        const mesExample = pickCharacterTextValue(character.mes_example, projectedData.mes_example, rawData.mes_example, rawCharacter.mes_example);
        const creator = pickCharacterTextValue(character.creator, projectedData.creator, rawData.creator, rawCharacter.creator);
        const creatorNotes = pickCharacterTextValue(character.creator_notes, projectedData.creator_notes, rawData.creator_notes, rawCharacter.creator_notes);
        const characterVersion = pickCharacterTextValue(character.character_version, projectedData.character_version, rawData.character_version, rawCharacter.character_version);
        const systemPrompt = pickCharacterTextValue(character.system_prompt, projectedData.system_prompt, rawData.system_prompt);
        const postHistoryInstructions = pickCharacterTextValue(
            character.post_history_instructions,
            projectedData.post_history_instructions,
            rawData.post_history_instructions,
        );

        const data = {
            ...rawData,
            ...projectedData,
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
            ...rawCharacter,
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

    /**
     * @param {any} avatar
     * @param {string} fieldName
     */
    function getExactAvatarInternalId(avatar, fieldName) {
        return characterStemFromAvatarFileName(avatar, fieldName, { required: true });
    }

    /** @param {any} avatar */
    function getOptionalAvatarInternalId(avatar) {
        if (!hasCharacterAvatarIdentity(avatar)) {
            return null;
        }

        return characterStemFromAvatarFileName(avatar, 'avatar_url', { required: true });
    }

    /**
     * @param {any} body
     * @param {string} fieldName
     */
    function hasBodyField(body, fieldName) {
        return Boolean(body && typeof body === 'object' && !Array.isArray(body)
            && Object.prototype.hasOwnProperty.call(body, fieldName));
    }

    /** @param {unknown} error */
    function isNotFoundError(error) {
        const message = error instanceof Error ? error.message : String(error || '');
        return /^\s*(not found:|entity not found:)/i.test(message);
    }

    /** @param {any} character */
    function getCharacterId(character) {
        if (!character || typeof character !== 'object') {
            return null;
        }

        if (typeof character.avatar === 'string') {
            try {
                const fromAvatar = characterStemFromAvatarFileName(character.avatar, 'avatar');
                if (fromAvatar) {
                    return fromAvatar;
                }
            } catch {
                // Keep list rendering tolerant of legacy non-file avatar sentinels such as "none".
            }
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

    function invalidateCharacterCache() {
        updateCharacterCache([]);
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
    function resolveCachedCharacterId(options = {}) {
        const avatar = options.avatar;
        const fallbackName = options.fallbackName;
        const avatarInternalId = getOptionalAvatarInternalId(avatar);

        if (hasCharacterAvatarIdentity(avatar)) {
            if (!avatarInternalId) {
                return null;
            }

            const fromRawAvatar = characterByAvatar.get(String(avatar));
            const fromRawAvatarId = getCharacterId(fromRawAvatar);
            if (fromRawAvatarId) {
                return fromRawAvatarId;
            }

            const fromInternalId = characterById.get(avatarInternalId);
            const fromInternalIdValue = getCharacterId(fromInternalId);
            if (fromInternalIdValue) {
                return fromInternalIdValue;
            }

            return null;
        }

        const fallback = String(fallbackName || '').trim();
        if (!fallback) {
            return null;
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

        return null;
    }

    /**
     * @param {{ avatar?: any; fallbackName?: string } | undefined} options
     */
    async function resolveExistingCharacterId(options = {}) {
        const avatar = options.avatar;
        const fallbackName = String(options.fallbackName || '').trim();
        if (!hasCharacterAvatarIdentity(avatar) && !fallbackName) {
            return null;
        }

        const avatarInternalId = getOptionalAvatarInternalId(avatar);
        if (avatarInternalId) {
            const character = await readCharacterById(avatarInternalId);
            return character ? avatarInternalId : null;
        }

        const cached = resolveCachedCharacterId(options);
        if (cached) {
            return cached;
        }

        await getAllCharacters({ shallow: true, forceRefresh: true });
        return resolveCachedCharacterId(options);
    }

    /**
     * @param {{ avatar?: any; fallbackName?: string } | undefined} options
     */
    async function resolveCharacterId(options = {}) {
        const avatarInternalId = getOptionalAvatarInternalId(options.avatar);
        if (avatarInternalId) {
            return avatarInternalId;
        }

        const fallback = String(options.fallbackName || '').trim();
        if (!fallback) {
            return null;
        }

        const existing = await resolveExistingCharacterId({ fallbackName: fallback });
        return existing || fallback;
    }

    /** @param {string} characterId */
    async function readCharacterById(characterId) {
        let character;
        try {
            character = await safeInvoke('get_character', { name: characterId });
        } catch (error) {
            if (isNotFoundError(error)) {
                return null;
            }
            throw error;
        }

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

    /** @param {any} body */
    async function getSingleCharacter(body) {
        let characterId = null;

        if (hasBodyField(body, 'avatar_url')) {
            characterId = getExactAvatarInternalId(body.avatar_url, 'avatar_url');
        } else if (hasBodyField(body, 'avatar')) {
            characterId = getExactAvatarInternalId(body.avatar, 'avatar');
        } else {
            characterId = String(body?.name || body?.ch_name || '').trim();
        }

        if (!characterId) {
            return null;
        }

        return readCharacterById(characterId);
    }

    /** @param {any} characterId */
    function findAvatarByCharacterId(characterId) {
        const key = String(characterId || '');
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

        try {
            const avatarFileName = assertCharacterAvatarFileName(key, 'characterId');
            const byAvatar = characterByAvatar.get(avatarFileName);
            if (byAvatar?.avatar) {
                return byAvatar.avatar;
            }
        } catch {
            // characterId is normally a storage stem; exact avatar filenames are accepted for callers that already have one.
        }

        const pngName = `${key}.png`;
        const byPng = characterByAvatar.get(pngName);
        if (byPng?.avatar) {
            return byPng.avatar;
        }

        return pngName;
    }

    return {
        normalizeCharacter,
        normalizeExtensions,
        getAllCharacters,
        invalidateCharacterCache,
        resolveCharacterId,
        resolveExistingCharacterId,
        getSingleCharacter,
        findAvatarByCharacterId,
    };
}
