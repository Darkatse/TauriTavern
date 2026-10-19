// @ts-check
import { createByteCommits } from './byte-commit.js';
import { characterAvatarFileNameFromStem } from '../../main/services/characters/character-identity.js';
import { readChatRecords } from '../commands/chat.js';

/** @param {ReturnType<import('../backend.js').createBackend>} backend @returns {import('../install.js').CommandHandlers} */
export function createChatCommits(backend) {
    const { request } = backend;
    const commits = createByteCommits();
    return {
        begin_chat_commit: ({ target, operation }) => {
            if (target.kind !== 'character' || operation.coldSourceId != null) {
                throw { Unsupported: 'Web Host supports character chat commits without cold swipe resources' };
            }
            return commits.begin({ target, operation });
        },
        append_chat_commit_chunk: commits.append,
        finish_chat_commit: args => {
            const { metadata: { target, operation }, text } = commits.finish(args);
            const address = { avatar_url: characterAvatarFileNameFromStem(target.characterId), file_name: target.fileName };
            const current = readChatRecords(backend, address);
            const integrity = current?.[0]?.chat_metadata?.integrity;
            let chat;
            let force = false;
            switch (operation.kind) {
                case 'payload':
                    chat = text.split('\n').filter(Boolean).map(line => JSON.parse(line));
                    force = operation.force;
                    break;
                case 'metadata':
                case 'metadataExtension': {
                    if (current === null || current.length === 0) throw { NotFound: `Chat not found: ${target.fileName}` };
                    chat = current;
                    const metadata = JSON.parse(text);
                    const header = chat[0];
                    if (operation.kind === 'metadata') {
                        header.chat_metadata = metadata;
                    } else {
                        const extensions = header.chat_metadata.extensions ??= {};
                        if (metadata === null) delete extensions[operation.namespace];
                        else extensions[operation.namespace] = metadata;
                    }
                    break;
                }
                default: throw { BadRequest: `Unknown chat commit operation: ${operation.kind}` };
            }
            if (!force && integrity != null && integrity !== chat[0]?.chat_metadata?.integrity) {
                throw { BadRequest: 'integrity' };
            }
            request('/api/chats/save', JSON.stringify({ ...address, chat, force }));
        },
        abort_chat_commit: commits.abort,
    };
}
