// @ts-check

import { isAndroidRuntime, isIosRuntime } from '../../../scripts/util/mobile-runtime.js';

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

function fileNameFromPath(filePath) {
    const value = requireNonEmptyString(filePath, 'character card file path');
    const path = value.startsWith('file://') ? decodeURIComponent(new URL(value).pathname) : value;
    const name = path.replace(/\\/g, '/').split('/').filter(Boolean).pop();
    return requireNonEmptyString(name, 'character card file name');
}

function normalizeDialogSelection(value) {
    if (value === null || value === undefined) {
        return null;
    }

    const values = Array.isArray(value) ? value : [value];
    return values.map(path => requireNonEmptyString(path, 'selected character card path'));
}

/**
 * @param {{
 *   safeInvoke: (command: string, args?: any) => Promise<any>;
 *   createReadableFileStream: (filePath: string) => ReadableStream<Uint8Array> | Promise<ReadableStream<Uint8Array>>;
 *   removeTemporaryFile?: (filePath: string) => Promise<void>;
 * }} deps
 */
function createCharacterCardsApi({
    safeInvoke,
    createReadableFileStream,
    removeTemporaryFile,
}) {
    if (typeof safeInvoke !== 'function') {
        throw new Error('Tauri main context safeInvoke is missing');
    }
    if (typeof createReadableFileStream !== 'function') {
        throw new Error('Tauri readable file stream service is missing');
    }

    async function fileFromPath(filePath, preferredName = '') {
        const fileName = preferredName ? requireNonEmptyString(preferredName, 'character card file name') : fileNameFromPath(filePath);
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

    function isNativePickerAvailable() {
        return !isAndroidRuntime();
    }

    async function cleanupIosPickedFile(filePath) {
        try {
            await removeTemporaryFile(filePath);
        } catch (error) {
            console.warn('Failed to cleanup iOS character card staging file:', error);
        }
    }

    async function pickIosFile() {
        if (typeof removeTemporaryFile !== 'function') {
            throw new Error('iOS character card picker cleanup is unavailable');
        }

        const result = await safeInvoke('ios_pick_character_card');
        if (result?.cancelled) {
            return null;
        }

        const filePath = requireNonEmptyString(result?.filePath ?? result?.file_path, 'iOS character card file path');
        const fileName = requireNonEmptyString(result?.fileName ?? result?.file_name, 'iOS character card file name');
        try {
            return [await fileFromPath(filePath, fileName)];
        } finally {
            await cleanupIosPickedFile(filePath);
        }
    }

    async function pickDesktopFiles({ multiple = false, title = 'Import Character Card' } = {}) {
        const selectedPaths = normalizeDialogSelection(await safeInvoke('plugin:dialog|open', {
            options: {
                title,
                multiple: Boolean(multiple),
                directory: false,
                filters: [
                    {
                        name: 'Character Card',
                        extensions: CHARACTER_CARD_EXTENSIONS,
                    },
                ],
            },
        }));

        if (!selectedPaths) {
            return null;
        }

        return Promise.all(selectedPaths.map(path => fileFromPath(path)));
    }

    async function pickFiles(options = {}) {
        if (!isNativePickerAvailable()) {
            return null;
        }

        if (isIosRuntime()) {
            if (options?.multiple) {
                throw new Error('iOS character card picker does not support multiple selection yet');
            }
            return pickIosFile();
        }

        return pickDesktopFiles(options);
    }

    return {
        isNativePickerAvailable,
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
        removeTemporaryFile: context?.removeTemporaryFile,
    });
}
