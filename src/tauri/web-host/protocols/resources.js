// @ts-check

export function createResources() {
    /** @type {Map<number, { bytes: Uint8Array, offset: number, chunkBytes: number }>} */
    const readers = new Map();
    let nextId = 0;
    /** @param {number} rid */
    function reader(rid) {
        const value = readers.get(rid);
        if (!value) throw new Error(`Unknown Web Host resource: ${rid}`);
        return value;
    }
    return {
        /** A byte reader that `read_bytes` drains `chunkBytes` at a time. @param {Uint8Array} bytes @param {number} chunkBytes */
        open(bytes, chunkBytes) {
            const rid = ++nextId;
            readers.set(rid, { bytes, offset: 0, chunkBytes });
            return rid;
        },
        commands: /** @type {import('../install.js').CommandHandlers} */ ({
            read_bytes: ({ rid }) => {
                const value = reader(rid);
                const chunk = value.bytes.slice(value.offset, value.offset + value.chunkBytes);
                value.offset += chunk.length;
                return chunk;
            },
            'plugin:resources|close': ({ rid }) => { reader(rid); readers.delete(rid); },
        }),
    };
}
