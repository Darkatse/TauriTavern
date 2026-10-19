// @ts-check
// Synchronous SHA-256 keeps the revision comparison and backend write in one task.
import { sha256 } from '../../../lib.js';
import { personaRecords } from '../../main/kernel/persona-projection.js';
import { characterAvatarFileNameFromStem } from '../../main/services/characters/character-identity.js';

/** @param {string} settings */
export function settingsRevision(settings) {
    return { hash_algorithm: 'web-host-settings-sha256-v1', settings_hash: sha256(settings) };
}

/** Keep ST's raw card body and metadata in the native command DTO. @param {any} character */
function characterDto(character) {
    return { ...character, extensions: character.data?.extensions, json_data: character.json_data ?? null };
}

/** @param {ReturnType<import('../backend.js').createBackend>} backend @param {string} name */
export function readCharacter({ json }, name) {
    const avatar = characterAvatarFileNameFromStem(name);
    const character = json('/api/characters/get', { avatar_url: avatar });
    if (typeof character.json_data !== 'string') throw new Error(`/api/characters/get (${avatar}): expected raw card text in json_data`);
    return characterDto(character);
}

/** @param {ReturnType<import('../backend.js').createBackend>} backend @returns {import('../install.js').CommandHandlers} */
export function createReadCommands(backend) {
    const { json } = backend;
    return {
        get_client_version: () => json('/version', undefined, 'GET'),
        get_bootstrap_metadata: () => ({
            characters: json('/api/characters/all').map(characterDto), groups: json('/api/groups/all'),
            avatars: json('/api/avatars/get'), secret_state: json('/api/secrets/read'),
        }),
        get_sillytavern_settings: () => {
            const response = json('/api/settings/get');
            response.tauritavern_settings_revision = settingsRevision(response.settings);
            return new TextEncoder().encode(JSON.stringify(response));
        },
        get_personas: () => personaRecords(JSON.parse(json('/api/settings/get').settings)),
        read_secret_state: () => json('/api/secrets/read'),
        get_character: ({ name }) => readCharacter(backend, name),
        get_character_chats_by_id: ({ dto }) => {
            const chats = json('/api/characters/chats', { avatar_url: characterAvatarFileNameFromStem(dto.name), simple: dto.simple });
            if (!Array.isArray(chats)) throw new Error('/api/characters/chats: expected an array');
            return chats.map(chat => ({
                file_name: chat.file_name, file_size: chat.file_size, chat_items: chat.chat_items,
                last_message: chat.mes, last_message_date: chat.last_mes,
            }));
        },
        get_all_backgrounds: () => json('/api/backgrounds/all').images,
        get_background_folders: () => json('/api/backgrounds/folders'),
        get_all_background_metadata: args => json('/api/image-metadata/all', args),
        get_world_info: ({ dto }) => new TextEncoder().encode(JSON.stringify(json('/api/worldinfo/get', dto))),
    };
}
