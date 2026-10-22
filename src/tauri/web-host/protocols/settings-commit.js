// @ts-check

import { createByteCommits } from './byte-commit.js';
import { settingsRevision } from '../commands/read.js';
import { personaProjection, personaRecords } from '../../main/kernel/persona-projection.js';

/** @param {ReturnType<import('../backend.js').createBackend>} backend @returns {import('../install.js').CommandHandlers} */
export function createSettingsCommits({ json, request }) {
    const commits = createByteCommits();
    return {
        begin_settings_commit: commits.begin,
        append_settings_commit_chunk: commits.append,
        finish_settings_commit: args => {
            const { metadata: { expectedRevision }, text } = commits.finish(args);
            const current = json('/api/settings/get');
            const revision = settingsRevision(current.settings);
            if (expectedRevision != null && (expectedRevision.hash_algorithm !== revision.hash_algorithm
                    || expectedRevision.settings_hash !== revision.settings_hash)) {
                throw { Conflict: 'Settings changed. Reload before saving again.' };
            }
            const payload = JSON.parse(text);
            // The registry is a delta of complete cards; omitted ids remain stored.
            const personas = { ...personaRecords(JSON.parse(current.settings)), ...personaRecords(payload) };
            payload.power_user = { ...payload.power_user, ...personaProjection(personas) };
            request('/api/settings/save', JSON.stringify(payload));
            return { result: 'ok', tauritavern_settings_revision: settingsRevision(json('/api/settings/get').settings), persona_errors: {} };
        },
        abort_settings_commit: commits.abort,
    };
}
