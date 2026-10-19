// @ts-check
import { characterAvatarFileNameFromStem } from '../../main/services/characters/character-identity.js';

/** @param {ReturnType<import('../backend.js').createBackend>} backend @param {ReturnType<import('../protocols/resources.js').createResources>} resources @returns {import('../install.js').CommandHandlers} */
export function createChatCommands({ json }, resources) {
    return {
        get_chat_payload_path: ({ characterName, fileName, allowNotFound }) => {
            const payload = json('/api/chats/get', { avatar_url: characterAvatarFileNameFromStem(characterName), file_name: fileName, allow_not_found: allowNotFound });
            if (Array.isArray(payload)) {
                return resources.path(new TextEncoder().encode(payload.map(record => JSON.stringify(record)).join('\n')));
            }
            if (payload !== null && typeof payload === 'object' && Object.keys(payload).length === 0) {
                if (allowNotFound) return '';
                throw { NotFound: `Chat not found: ${characterName}/${fileName}` };
            }
            throw new Error(`Invalid chat payload for ${characterName}/${fileName}: expected records or {}`);
        },
    };
}
