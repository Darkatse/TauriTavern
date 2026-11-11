// @ts-check
import { BYTE_HEADERS } from '../../main/kernel/byte-protocol.js';
import { decodeBase64ToBytes } from '../../main/binary-utils.js';

/** Byte framing shared by settings and chat commits; each protocol owns its sessions. */
export function createByteCommits() {
    const maxFrameBytes = 1024 * 1024;
    /** @type {Map<string, { metadata: any, size: number, chunks: Uint8Array[] }>} */
    const sessions = new Map();
    /** @param {string} id */
    function get(id) {
        const value = sessions.get(id);
        if (!value) throw { NotFound: `Unknown byte commit: ${id}` };
        return value;
    }
    return {
        /** @param {any} metadata */
        begin(metadata) {
            const sessionId = crypto.randomUUID();
            sessions.set(sessionId, { metadata: structuredClone(metadata), size: 0, chunks: [] });
            return { sessionId, maxFrameBytes };
        },
        /** @param {any} body @param {{ headers: Record<string, string> }} options */
        append(body, { headers }) {
            const value = get(/** @type {string} */ (headers[BYTE_HEADERS.sessionId]));
            const bytes = headers[BYTE_HEADERS.encoding] === 'base64' ? decodeBase64ToBytes(body.data) : body;
            if (Number(headers[BYTE_HEADERS.offset]) !== value.size || bytes.byteLength > maxFrameBytes) {
                throw { BadRequest: 'Invalid byte commit chunk' };
            }
            value.chunks.push(bytes.slice());
            value.size += bytes.byteLength;
            return value.size;
        },
        /** @param {{ sessionId: string, expectedSize: number }} args */
        finish({ sessionId, expectedSize }) {
            const value = get(sessionId);
            sessions.delete(sessionId);
            if (value.size !== expectedSize) throw { BadRequest: 'Byte commit size mismatch' };
            const bytes = new Uint8Array(value.size);
            let offset = 0;
            for (const chunk of value.chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
            return { metadata: value.metadata, text: new TextDecoder().decode(bytes) };
        },
        /** @param {{ sessionId: string }} args */
        abort({ sessionId }) { sessions.delete(sessionId); },
    };
}
