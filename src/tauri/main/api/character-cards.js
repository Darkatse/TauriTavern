// @ts-check

import { commandErrorStatus, extractErrorText } from '../../../scripts/util/command-error-utils.js';
import { StagedFile } from '../services/files/staged-file.js';
import { expectCharacter, stageUrlContent, takeUrlContent } from '../services/files/url-content.js';
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
 * @param {Pick<import('../context/types.js').TauriMainContext,
 *   'safeInvoke' | 'invalidateCharacterCache' | 'adoptStagedFile' | 'createByteStream'>} context
 */
function createCharacterCardsApi({
    safeInvoke,
    invalidateCharacterCache,
    adoptStagedFile,
    createByteStream,
}) {
    /** @param {{ multiple?: boolean }} [options] */
    async function pickFiles({ multiple = false } = {}) {
        const selected = await safeInvoke('pick_import_files', { kind: 'character-card', multiple });
        return selected === null ? null : selected.map(stagedFile);
    }

    /** @param {string} url */
    async function stageFromUrl(url) {
        return stagedFile(expectCharacter(await stageUrlContent(safeInvoke, url)));
    }

    /**
     * @param {string} url
     * @returns {Promise<TauriTavernUrlContent>}
     */
    async function contentFromUrl(url) {
        const content = await stageUrlContent(safeInvoke, url);
        if (content.kind === 'character') {
            return { kind: content.kind, file: stagedFile(content) };
        }
        const { bytes, type } = await takeUrlContent({ safeInvoke, adoptStagedFile, createByteStream }, content);
        return { kind: content.kind, file: new File([bytes], content.name, { type }) };
    }

    /** @param {string} id @param {string} name */
    async function stageSharedFile(id, name) {
        return stagedFile(await safeInvoke('stage_shared_file', { id, name }));
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
            if (commandErrorStatus(error) === 409) {
                throw new LorebookAdoptionConflictError(extractErrorText(error));
            }
            throw error;
        }
    }

    return {
        pickFiles,
        stageFromUrl,
        contentFromUrl,
        stageSharedFile,
        adoptLorebook,
        resolveLorebookAdoption,
    };
}

/**
 * @param {TauriTavernHostApi} api
 * @param {import('../context/types.js').TauriMainContext} context
 */
export function installCharacterCardsApi(api, context) {
    api.characterCards = createCharacterCardsApi(context);
}
