// @ts-check
import { characterAvatarFileNameFromStem } from '../../main/services/characters/character-identity.js';

/** Read ST's records-or-missing response once for both reads and commits. @param {ReturnType<import('../backend.js').createBackend>} backend @param {any} address */
export function readChatRecords({ json }, address) {
    const payload = json('/api/chats/get', address);
    if (Array.isArray(payload)) return payload;
    if (payload !== null && typeof payload === 'object' && Object.keys(payload).length === 0) return null;
    throw new Error(`Invalid chat payload for ${address.avatar_url}/${address.file_name}: expected records or {}`);
}

/** @param {ReturnType<import('../backend.js').createBackend>} backend @param {ReturnType<import('../protocols/resources.js').createResources>} resources @returns {import('../install.js').CommandHandlers} */
export function createChatCommands(backend, resources) {
    return {
        get_chat_payload_path: ({ characterName, fileName, allowNotFound }) => {
            const payload = readChatRecords(backend, { avatar_url: characterAvatarFileNameFromStem(characterName), file_name: fileName });
            if (payload !== null) {
                return resources.path(new TextEncoder().encode(payload.map(record => JSON.stringify(record)).join('\n')));
            }
            if (allowNotFound) return '';
            throw { NotFound: `Chat not found: ${characterName}/${fileName}` };
        },
    };
}
