// @ts-check
import { characterAvatarFileNameFromStem } from '../../main/services/characters/character-identity.js';
import { readCharacter } from './read.js';

/** @param {ReturnType<import('../backend.js').createBackend>} backend @returns {import('../install.js').CommandHandlers} */
export function createWriteCommands(backend) {
    const { request } = backend;
    return {
        save_theme: ({ dto }) => { request('/api/themes/save', JSON.stringify(dto)); },
        save_quick_reply_set: ({ payload }) => { request('/api/quick-replies/save', JSON.stringify(payload)); },
        update_character_card_data: ({ name, dto }) => {
            if (dto.avatar_path != null || dto.crop != null) throw { Unsupported: 'Web Host does not edit character avatars' };
            const card = JSON.parse(dto.card_json);
            const data = card.data;
            const extensions = data.extensions ?? {};
            const fields = {
                ...Object.fromEntries(Object.entries(data).filter(([, value]) => typeof value === 'string')),
                ch_name: data.name, avatar_url: characterAvatarFileNameFromStem(name),
                chat: card.chat, create_date: card.create_date, json_data: dto.card_json,
                tags: (data.tags ?? []).join(','),
                fav: String(extensions.fav ?? false), talkativeness: String(extensions.talkativeness ?? 0.5),
                world: extensions.world ?? '',
                depth_prompt_prompt: extensions.depth_prompt?.prompt ?? '',
                depth_prompt_depth: String(extensions.depth_prompt?.depth ?? 4),
                depth_prompt_role: extensions.depth_prompt?.role ?? 'system',
            };
            /** @type {import('../install.js').FormEntries['entries']} */
            const entries = [];
            for (const [key, value] of Object.entries(fields)) {
                if (value !== undefined) entries.push([key, String(value)]);
            }
            for (const greeting of data.alternate_greetings ?? []) entries.push(['alternate_greetings', greeting]);
            request('/api/characters/edit', { kind: 'form', entries });
            return readCharacter(backend, name);
        },
        merge_character_card_data: ({ name, dto }) => {
            request('/api/characters/merge-attributes', JSON.stringify({ ...dto.update, avatar: characterAvatarFileNameFromStem(name) }));
            return readCharacter(backend, name);
        },
    };
}
