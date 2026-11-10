// @ts-check

import { StagedFile } from '../services/files/staged-file.js';
import { characterStemFromAvatarFileName } from '../services/characters/character-identity.js';

export class LorebookAdoptionConflictError extends Error {
    /** @param {string} message */
    constructor(message) {
        super(message);
        this.name = 'LorebookAdoptionConflictError';
    }
}

/** @param {{ path: string; name: string }} staged */
function stagedFile({ path, name }) {
    return new StagedFile(path, name);
}

/**
 * @param {{
 *   safeInvoke: (command: string, args?: any) => Promise<any>;
 *   invalidateCharacterCache: () => void;
 * }} deps
 */
function createCharacterCardsApi({
    safeInvoke,
    invalidateCharacterCache,
}) {
    if (typeof safeInvoke !== 'function') {
        throw new Error('Tauri main context safeInvoke is missing');
    }
    if (typeof invalidateCharacterCache !== 'function') {
        throw new Error('Tauri main context invalidateCharacterCache is missing');
    }

    /** @param {{ multiple?: boolean }} [options] */
    async function pickFiles({ multiple = false } = {}) {
        const selected = await safeInvoke('pick_import_files', { kind: 'character-card', multiple });
        return selected === null ? null : selected.map(stagedFile);
    }

    /** @param {string} url */
    async function stageFromUrl(url) {
        return stagedFile(await safeInvoke('stage_character_card_from_url', { url }));
    }

    /** @param {string} uri @param {string} name */
    async function stageSharedFile(uri, name) {
        return stagedFile(await safeInvoke('stage_shared_file', { uri, name }));
    }

    /** @param {string} avatar */
    async function adoptLorebook(avatar) {
        const name = characterStemFromAvatarFileName(avatar, 'avatar', { required: true });
        const result = await safeInvoke('adopt_character_lorebook', { dto: { name } });
        invalidateCharacterCache();
        return result;
    }

    /**
     * @param {string} avatar
     * @param {'adopt' | 'update'} choice
     * @param {string} token
     */
    async function resolveLorebookAdoption(avatar, choice, token) {
        const name = characterStemFromAvatarFileName(avatar, 'avatar', { required: true });
        try {
            const result = await safeInvoke('resolve_character_lorebook_adoption', { dto: { name, choice, token } });
            invalidateCharacterCache();
            return result;
        } catch (error) {
            if (error instanceof Error && error.message.startsWith('Conflict:')) {
                throw new LorebookAdoptionConflictError(error.message);
            }
            throw error;
        }
    }

    return {
        pickFiles,
        stageFromUrl,
        stageSharedFile,
        adoptLorebook,
        resolveLorebookAdoption,
    };
}

/**
 * @param {any} context
 */
export function installCharacterCardsApi(context) {
    const hostWindow = /** @type {any} */ (window);
    const hostAbi = hostWindow.__TAURITAVERN__;
    if (!hostAbi || typeof hostAbi !== 'object') {
        throw new Error('Host ABI __TAURITAVERN__ is missing');
    }

    if (!hostAbi.api || typeof hostAbi.api !== 'object') {
        hostAbi.api = {};
    }

    hostAbi.api.characterCards = createCharacterCardsApi({
        safeInvoke: context?.safeInvoke,
        invalidateCharacterCache: context?.invalidateCharacterCache,
    });
}
