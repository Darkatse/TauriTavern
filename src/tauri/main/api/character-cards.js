// @ts-check

import { createFileStagingService } from '../services/files/file-staging-service.js';
import { characterStemFromAvatarFileName } from '../services/characters/character-identity.js';

const CHARACTER_CARD_EXTENSIONS = ['json', 'png'];
const CHARACTER_CARD_EXTENSION_SET = new Set(CHARACTER_CARD_EXTENSIONS);
const CHARACTER_CARD_MIME_BY_EXTENSION = new Map([
    ['json', 'application/json'],
    ['png', 'image/png'],
]);

export class LorebookAdoptionConflictError extends Error {
    /** @param {string} message */
    constructor(message) {
        super(message);
        this.name = 'LorebookAdoptionConflictError';
    }
}

/** @param {unknown} value @param {string} label */
function requireNonEmptyString(value, label) {
    const normalized = String(value ?? '').trim();
    if (!normalized) {
        throw new Error(`${label} is required`);
    }
    return normalized;
}

/** @param {string} fileName */
function characterCardExtension(fileName) {
    const match = String(fileName || '').match(/\.([^.]+)$/);
    const extension = match?.[1]?.toLowerCase() || '';
    if (!CHARACTER_CARD_EXTENSION_SET.has(extension)) {
        throw new Error(`Unsupported character card file type: ${fileName}`);
    }
    return extension;
}

/**
 * @param {{
 *   safeInvoke: (command: string, args?: any) => Promise<any>;
 *   invalidateCharacterCache: () => void;
 *   createReadableFileStream: (filePath: string) => ReadableStream<Uint8Array> | Promise<ReadableStream<Uint8Array>>;
 * }} deps
 */
function createCharacterCardsApi({
    safeInvoke,
    invalidateCharacterCache,
    createReadableFileStream,
}) {
    if (typeof safeInvoke !== 'function') {
        throw new Error('Tauri main context safeInvoke is missing');
    }
    if (typeof invalidateCharacterCache !== 'function') {
        throw new Error('Tauri main context invalidateCharacterCache is missing');
    }
    if (typeof createReadableFileStream !== 'function') {
        throw new Error('Tauri readable file stream service is missing');
    }

    const staging = createFileStagingService({ safeInvoke });

    /** @param {string} filePath @param {string} preferredName */
    async function fileFromPath(filePath, preferredName) {
        const fileName = requireNonEmptyString(preferredName, 'character card file name');
        const extension = characterCardExtension(fileName);
        const type = CHARACTER_CARD_MIME_BY_EXTENSION.get(extension) || 'application/octet-stream';
        const stream = await createReadableFileStream(filePath);
        return new File([await new Response(stream).blob()], fileName, { type });
    }

    async function pickFiles({ multiple = false } = {}) {
        const selected = await safeInvoke('pick_import_files', { kind: 'character-card', multiple });
        if (selected === null) return null;
        try {
            const files = [];
            for (const { path, name } of selected) files.push(await fileFromPath(path, name));
            return files;
        } finally {
            for (const { path } of selected) await staging.discardFile(path);
        }
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
        isNativePickerAvailable: () => true,
        pickFiles,
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
        createReadableFileStream: context?.createReadableFileStream,
    });
}
