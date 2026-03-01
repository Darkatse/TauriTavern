import { invoke, isTauriEnv } from '../../../tauri-bridge.js';
import { fetchAssetStream, writeTempFileFromBytesIterable } from './asset-io.js';
import { jsonlStreamToPayload, payloadToJsonlByteChunks } from './jsonl.js';

function normalizeChatFileName(fileName) {
    const value = String(fileName || '').trim();
    if (!value) {
        return '';
    }

    return value.replace(/\.jsonl$/i, '');
}

function normalizeAvatarFileName(avatar) {
    if (avatar === null || avatar === undefined) {
        return null;
    }

    let value = String(avatar).trim();
    if (!value) {
        return null;
    }

    if (value.includes('?')) {
        const parsed = new URL(value, 'http://localhost');
        value = parsed.searchParams.get('file') || parsed.pathname || value;
    }

    value = decodeURIComponent(value);

    value = value.split('?')[0].split('#')[0];
    if (!value) {
        return null;
    }

    const normalized = value.replace(/[\\/]+/g, '/');
    const fileName = normalized.split('/').pop();
    return fileName || null;
}

function getAvatarInternalId(avatar) {
    const fileName = normalizeAvatarFileName(avatar);
    if (!fileName) {
        return null;
    }

    return fileName.replace(/\.[^/.]+$/, '') || null;
}

function resolveCharacterDirectoryId(characterName, avatarUrl) {
    const fromAvatar = getAvatarInternalId(avatarUrl);
    if (fromAvatar) {
        return fromAvatar;
    }

    return String(characterName || '').trim();
}

async function withTempFile(bytesIterable, options, handler) {
    const tempFile = await writeTempFileFromBytesIterable(bytesIterable, options);

    let result;
    let handlerError;

    try {
        result = await handler(tempFile.filePath);
    } catch (error) {
        handlerError = error;
    }

    try {
        await tempFile.cleanup();
    } catch (cleanupError) {
        if (handlerError) {
            const handlerMessage = String(handlerError?.message || handlerError || 'Temp file handler failed');
            throw new AggregateError([handlerError, cleanupError], handlerMessage);
        }

        throw cleanupError;
    }

    if (handlerError) {
        throw handlerError;
    }

    return result;
}

export function isTauriChatPayloadTransportEnabled() {
    return isTauriEnv;
}

export async function loadCharacterChatPayload({ characterName, avatarUrl, fileName, allowNotFound = true }) {
    const normalizedCharacter = resolveCharacterDirectoryId(characterName, avatarUrl);
    const normalizedFile = normalizeChatFileName(fileName);
    if (!normalizedCharacter || !normalizedFile) {
        return [];
    }

    const path = await invoke('get_chat_payload_path', {
        characterName: normalizedCharacter,
        fileName: normalizedFile,
        allowNotFound,
    });

    if (!path) {
        if (allowNotFound) {
            return [];
        }
        throw new Error('Chat payload path is empty');
    }

    const stream = await fetchAssetStream(path);
    return jsonlStreamToPayload(stream);
}

export async function saveCharacterChatPayload({ characterName, avatarUrl, fileName, payload, force = false }) {
    const normalizedCharacter = resolveCharacterDirectoryId(characterName, avatarUrl);
    const normalizedFile = normalizeChatFileName(fileName);
    if (!Array.isArray(payload) || payload.length === 0 || !normalizedCharacter || !normalizedFile) {
        throw new Error('Invalid chat payload');
    }

    await withTempFile(payloadToJsonlByteChunks(payload), {
        prefix: 'tauritavern-chat',
        extension: 'jsonl',
    }, (filePath) => invoke('save_chat_payload_from_file', {
        dto: {
            ch_name: normalizedCharacter,
            file_name: normalizedFile,
            file_path: filePath,
            force,
        },
    }));
}

export async function loadGroupChatPayload({ id, allowNotFound = true }) {
    const normalizedId = normalizeChatFileName(id);
    if (!normalizedId) {
        return [];
    }

    const path = await invoke('get_group_chat_path', {
        id: normalizedId,
        allowNotFound,
    });

    if (!path) {
        if (allowNotFound) {
            return [];
        }
        throw new Error('Group chat payload path is empty');
    }

    const stream = await fetchAssetStream(path);
    return jsonlStreamToPayload(stream);
}

export async function saveGroupChatPayload({ id, payload, force = false }) {
    const normalizedId = normalizeChatFileName(id);
    if (!Array.isArray(payload) || payload.length === 0 || !normalizedId) {
        throw new Error('Invalid group chat payload');
    }

    await withTempFile(payloadToJsonlByteChunks(payload), {
        prefix: 'tauritavern-group-chat',
        extension: 'jsonl',
    }, (filePath) => invoke('save_group_chat_from_file', {
        dto: {
            id: normalizedId,
            file_path: filePath,
            force,
        },
    }));
}
