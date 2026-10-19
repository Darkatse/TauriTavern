// @ts-check

import { BYTE_HEADERS } from '../../main/kernel/byte-protocol.js';
import { decodeBase64ToBytes } from '../../main/binary-utils.js';
import { settingsRevision } from '../commands/read.js';
import { personaProjection, personaRecords } from '../../main/kernel/persona-projection.js';

/** @param {ReturnType<import('../backend.js').createBackend>} backend @returns {import('../install.js').CommandHandlers} */
export function createSettingsCommits({ json }) {
    const maxFrameBytes = 1024 * 1024;
    /** @type {Map<string, { expectedRevision: any, bytes: number, chunks: Uint8Array[] }>} */
    const sessions = new Map();
    /** @param {string} id */
    function session(id) {
        const value = sessions.get(id);
        if (!value) throw new Error(`Unknown settings commit: ${id}`);
        return value;
    }
    return {
        begin_settings_commit: ({ expectedRevision }) => {
            const sessionId = crypto.randomUUID();
            sessions.set(sessionId, { expectedRevision, bytes: 0, chunks: [] });
            return { sessionId, maxFrameBytes };
        },
        append_settings_commit_chunk: (body, { headers }) => {
            const value = session(headers[BYTE_HEADERS.sessionId]);
            const bytes = headers[BYTE_HEADERS.encoding] === 'base64' ? decodeBase64ToBytes(body.data) : body;
            if (Number(headers[BYTE_HEADERS.offset]) !== value.bytes || bytes.byteLength > maxFrameBytes) {
                throw { BadRequest: 'Invalid settings commit chunk' };
            }
            value.chunks.push(bytes.slice());
            value.bytes += bytes.byteLength;
            return value.bytes;
        },
        finish_settings_commit: ({ sessionId, expectedSize }) => {
            const value = session(sessionId);
            sessions.delete(sessionId);
            if (value.bytes !== expectedSize) throw { BadRequest: 'Settings commit size mismatch' };
            const current = json('/api/settings/get');
            const revision = settingsRevision(current.settings);
            if (value.expectedRevision !== null && value.expectedRevision !== undefined
                && (value.expectedRevision.hash_algorithm !== revision.hash_algorithm
                    || value.expectedRevision.settings_hash !== revision.settings_hash)) {
                throw { Conflict: 'Settings changed. Reload before saving again.' };
            }
            const bytes = new Uint8Array(value.bytes);
            let offset = 0;
            for (const chunk of value.chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
            const payload = JSON.parse(new TextDecoder().decode(bytes));
            // The registry is a delta of complete cards; omitted ids remain stored.
            const personas = { ...personaRecords(JSON.parse(current.settings)), ...personaRecords(payload) };
            payload.power_user = { ...payload.power_user, ...personaProjection(personas) };
            json('/api/settings/save', payload);
            return { result: 'ok', tauritavern_settings_revision: settingsRevision(json('/api/settings/get').settings), persona_errors: {} };
        },
        abort_settings_commit: ({ sessionId }) => { sessions.delete(sessionId); },
    };
}
