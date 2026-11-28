// @ts-check

import { createByteCommits } from './byte-commit.js';

// The native reader's chunk size, so an entry takes as many reads.
const READ_CHUNK_BYTES = 512 * 1024;

/**
 * Extension store entries, kept in the store document. Only JSON entries are ever written, so only they are found.
 * @param {import('../install.js').HostStatePart} part
 * @param {ReturnType<import('./resources.js').createResources>} resources
 * @returns {import('../install.js').CommandHandlers}
 */
export function createExtensionStore(part, resources) {
    const commits = createByteCommits();
    /** @param {{ namespace: string, table?: string, key: string }} entry @param {string} kind */
    const id = ({ namespace, table, key }, kind) => JSON.stringify([namespace, table ?? 'main', key, kind]);
    /** @returns {Record<string, string>} */
    const entries = () => part.load() ?? {};
    return {
        open_extension_store_entry: ({ kind, ...entry }) => {
            const text = entries()[id(entry, kind)];
            if (text === undefined) return null;
            return { readerId: resources.open(new TextEncoder().encode(text), READ_CHUNK_BYTES), mimeType: 'application/json' };
        },
        begin_extension_store_commit: ({ operation, ...entry }) => {
            if (operation !== 'setJson') throw { Unsupported: `Web Host does not support extension store ${operation}` };
            return commits.begin({ id: id(entry, 'json') });
        },
        append_extension_store_commit_chunk: commits.append,
        finish_extension_store_commit: args => {
            const { metadata, text } = commits.finish(args);
            part.save({ ...entries(), [metadata.id]: text });
        },
        abort_extension_store_commit: commits.abort,
    };
}
