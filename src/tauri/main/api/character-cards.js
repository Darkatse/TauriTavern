// @ts-check

import { createFileStagingService } from '../services/files/file-staging-service.js';

const CHARACTER_CARD_EXTENSIONS = ['json', 'png'];
const CHARACTER_CARD_EXTENSION_SET = new Set(CHARACTER_CARD_EXTENSIONS);
const CHARACTER_CARD_MIME_BY_EXTENSION = new Map([
    ['json', 'application/json'],
    ['png', 'image/png'],
]);

function requireNonEmptyString(value, label) {
    const normalized = String(value ?? '').trim();
    if (!normalized) {
        throw new Error(`${label} is required`);
    }
    return normalized;
}

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
 *   createReadableFileStream: (filePath: string) => ReadableStream<Uint8Array> | Promise<ReadableStream<Uint8Array>>;
 * }} deps
 */
function createCharacterCardsApi({
    safeInvoke,
    createReadableFileStream,
}) {
    if (typeof safeInvoke !== 'function') {
        throw new Error('Tauri main context safeInvoke is missing');
    }
    if (typeof createReadableFileStream !== 'function') {
        throw new Error('Tauri readable file stream service is missing');
    }

    const staging = createFileStagingService({ safeInvoke });

    async function fileFromPath(filePath, preferredName) {
        const fileName = requireNonEmptyString(preferredName, 'character card file name');
        const extension = characterCardExtension(fileName);
        const type = CHARACTER_CARD_MIME_BY_EXTENSION.get(extension) || 'application/octet-stream';
        const stream = await createReadableFileStream(filePath);
        const reader = stream.getReader();
        const chunks = [];

        try {
            for (;;) {
                const { done, value } = await reader.read();
                if (done) {
                    break;
                }
                if (value) {
                    chunks.push(value);
                }
            }
        } finally {
            reader.releaseLock();
        }

        return new File(chunks, fileName, { type });
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

    return {
        isNativePickerAvailable: () => true,
        pickFiles,
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
        createReadableFileStream: context?.createReadableFileStream,
    });
}
