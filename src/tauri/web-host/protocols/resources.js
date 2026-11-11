// @ts-check

import { FILE_READ_COUNT_BYTES } from '../../main/kernel/byte-protocol.js';

export function createResources() {
    /** @type {Map<number, { bytes: Uint8Array, offset: number }>} */
    const readers = new Map();
    /** @type {Map<string, Uint8Array>} */
    const paths = new Map();
    let nextId = 0;
    /** @param {number} rid */
    function reader(rid) {
        const value = readers.get(rid);
        if (!value) throw new Error(`Unknown Web Host resource: ${rid}`);
        return value;
    }
    return {
        /** @param {Uint8Array} bytes */
        path(bytes) {
            const path = `web-host:chat/${++nextId}`;
            paths.set(path, bytes);
            return path;
        },
        commands: /** @type {import('../install.js').CommandHandlers} */ ({
            'plugin:fs|open': ({ path }) => {
                const bytes = paths.get(path);
                if (!bytes) throw new Error(`Unknown Web Host file: ${path}`);
                paths.delete(path);
                const rid = ++nextId;
                readers.set(rid, { bytes, offset: 0 });
                return rid;
            },
            'plugin:fs|fstat': ({ rid }) => ({ size: reader(rid).bytes.length }),
            'plugin:fs|read': ({ rid, len }) => {
                const value = reader(rid);
                const chunk = value.bytes.subarray(value.offset, value.offset + len);
                value.offset += chunk.length;
                const response = new Uint8Array(chunk.length + FILE_READ_COUNT_BYTES);
                response.set(chunk);
                new DataView(response.buffer).setBigUint64(chunk.length, BigInt(chunk.length));
                return response;
            },
            'plugin:resources|close': ({ rid }) => { reader(rid); readers.delete(rid); },
        }),
    };
}
