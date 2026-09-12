import { invoke } from '../../../tauri-bridge.js';
import { stripJsonl } from '../../../tauri/main/kernel/chat-utils.js';
import {
    characterStemFromAvatarFileName,
    hasCharacterAvatarIdentity,
} from '../../../tauri/main/services/characters/character-identity.js';
import { createReadableFileStreamService } from '../../../tauri/main/services/files/readable-file-stream-service.js';
import { commitChatMetadata, commitChatPayload } from './commit.js';
import { jsonlStreamToPayload } from './jsonl.js';

const { createReadableFileStream } = createReadableFileStreamService({ invoke });

export const CHAT_COMMIT_REASON = Object.freeze({
    MUTATION: 'mutation',
    PROVIDER_BARRIER: 'providerBarrier',
    GENERATION_CHECKPOINT: 'generationCheckpoint',
    MAINTENANCE: 'maintenance',
});

export function normalizeChatFileName(fileName) {
    return stripJsonl(fileName);
}

/**
 * Chat folders are keyed by the character avatar filename stem.
 * avatarUrl is SillyTavern's avatar_url API field, not a browser asset URL.
 */
export function resolveCharacterDirectoryId(characterName, avatarUrl) {
    if (hasCharacterAvatarIdentity(avatarUrl)) {
        return characterStemFromAvatarFileName(avatarUrl, 'avatar_url', { required: true });
    }

    return String(characterName || '').trim();
}

function characterTarget({ characterName, avatarUrl, fileName }) {
    const characterId = resolveCharacterDirectoryId(characterName, avatarUrl);
    const normalizedFile = normalizeChatFileName(fileName);
    if (!characterId || !normalizedFile.trim()) {
        throw new Error('Invalid character chat target');
    }
    return { kind: 'character', characterId, fileName: normalizedFile };
}

function groupTarget(id) {
    const chatId = normalizeChatFileName(id);
    if (!chatId.trim()) {
        throw new Error('Invalid group chat target');
    }
    return { kind: 'group', chatId };
}

export async function loadCharacterChatPayload({ characterName, avatarUrl, fileName, allowNotFound = false }) {
    const target = characterTarget({ characterName, avatarUrl, fileName });
    const path = await invoke('get_chat_payload_path', {
        characterName: target.characterId,
        fileName: target.fileName,
        allowNotFound,
    });

    if (!path) {
        if (allowNotFound) {
            return [];
        }
        throw new Error('Chat payload path is empty');
    }

    const stream = createReadableFileStream(path);
    return jsonlStreamToPayload(stream);
}

export async function saveCharacterChatPayload({ characterName, avatarUrl, fileName, payload, force = false, commitReason = CHAT_COMMIT_REASON.MUTATION }) {
    await commitChatPayload({ target: characterTarget({ characterName, avatarUrl, fileName }), payload, force, commitReason });
}

export async function saveCharacterChatMetadata({ characterName, avatarUrl, fileName, chatMetadata }) {
    await commitChatMetadata({ target: characterTarget({ characterName, avatarUrl, fileName }), chatMetadata });
}

export async function loadGroupChatPayload({ id, allowNotFound = false }) {
    const target = groupTarget(id);
    const path = await invoke('get_group_chat_path', { id: target.chatId, allowNotFound });

    if (!path) {
        if (allowNotFound) {
            return [];
        }
        throw new Error('Group chat payload path is empty');
    }

    const stream = createReadableFileStream(path);
    return jsonlStreamToPayload(stream);
}

export async function saveGroupChatPayload({ id, payload, force = false, commitReason = CHAT_COMMIT_REASON.MUTATION }) {
    await commitChatPayload({ target: groupTarget(id), payload, force, commitReason });
}

export async function saveGroupChatMetadata({ id, chatMetadata }) {
    await commitChatMetadata({ target: groupTarget(id), chatMetadata });
}
