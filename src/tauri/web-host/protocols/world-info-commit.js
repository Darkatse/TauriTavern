// @ts-check

import { createByteCommits } from './byte-commit.js';

/** @param {ReturnType<import('../backend.js').createBackend>} backend @returns {import('../install.js').CommandHandlers} */
export function createWorldInfoCommits({ request }) {
    const commits = createByteCommits();
    return {
        begin_world_info_commit: commits.begin,
        append_world_info_commit_chunk: commits.append,
        // The committed bytes are the `{ name, data }` body that ST's edit endpoint takes.
        finish_world_info_commit: args => { request('/api/worldinfo/edit', commits.finish(args).text); },
        abort_world_info_commit: commits.abort,
    };
}
