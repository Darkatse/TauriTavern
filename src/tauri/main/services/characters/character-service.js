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
        resolveCharacterId,
        resolveExistingCharacterId,
        getSingleCharacter,
        findAvatarByCharacterId,
    };
}
