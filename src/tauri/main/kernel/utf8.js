const textEncoder = new TextEncoder();

/** Encode immutable text fragments without allocating a complete UTF-8 document. */
export function* textFragmentsToByteChunks(fragments, maxChunkBytes, separator = '') {
    if (!Number.isSafeInteger(maxChunkBytes) || maxChunkBytes < 4) {
        throw new Error('maxChunkBytes must fit at least one UTF-8 character (4 bytes)');
    }

    // Size small commits by their UTF-8 upper bound: three bytes per UTF-16 code unit.
    const upperBound = fragments.reduce((size, text) => size + (text.length + separator.length) * 3, 0);
    const capacity = Math.min(maxChunkBytes, Math.max(4, upperBound));
    let frame = new Uint8Array(capacity);
    let used = 0;
    let first = true;
    for (const fragment of fragments) {
        for (const text of first ? [fragment] : [separator, fragment]) {
            let offset = 0;
            while (offset < text.length) {
                if (frame.length - used < 4) {
                    yield frame.subarray(0, used);
                    frame = new Uint8Array(capacity);
                    used = 0;
                }
                let end = Math.min(offset + frame.length - used, text.length);
                // A bounded substring must not split a UTF-16 surrogate pair.
                const last = text.charCodeAt(end - 1);
                if (end < text.length && last >= 0xD800 && last <= 0xDBFF) end -= 1;
                const { read, written } = textEncoder.encodeInto(
                    text.slice(offset, end),
                    frame.subarray(used),
                );
                offset += read;
                used += written;
            }
        }
        first = false;
    }
    if (used > 0) yield frame.subarray(0, used);
}
