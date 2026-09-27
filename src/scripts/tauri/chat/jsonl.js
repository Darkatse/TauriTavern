const textEncoder = new TextEncoder();

function isJsonWhitespace(code) {
    return code === 0x20 || code === 0x09 || code === 0x0A || code === 0x0D;
}

function trimJsonWhitespaceRange(value, start = 0) {
    let end = value.length;
    while (start < end && isJsonWhitespace(value.charCodeAt(start))) start += 1;
    while (end > start && isJsonWhitespace(value.charCodeAt(end - 1))) end -= 1;
    return [start, end];
}

function assertHeader(header) {
    const metadata = header.chat_metadata;
    if (!metadata || !Object.hasOwn(metadata, 'integrity')) return;

    const value = metadata.integrity;
    if (typeof value !== 'string' || value.length === 0) {
        throw new Error('Chat header integrity must be a non-empty string');
    }
}

/** Parses one record; message slices have no header or BOM preamble of their own. */
export function parseJsonlRecord(text, lineNumber) {
    try {
        const record = JSON.parse(text);
        if (!record || typeof record !== 'object' || Array.isArray(record)) {
            throw new Error('Chat record must be an object');
        }
        return record;
    } catch (error) {
        throw new Error(`Invalid JSONL at line ${lineNumber}`, { cause: error });
    }
}

function createJsonlParser(hasHeader = true) {
    let headerPending = hasHeader;
    let bomConsumed = false;
    let lineNumber = 0;

    return (line) => {
        lineNumber += 1;
        let [start, end] = trimJsonWhitespaceRange(line);
        if (headerPending && !bomConsumed && line.charCodeAt(start) === 0xFEFF) {
            bomConsumed = true;
            [start, end] = trimJsonWhitespaceRange(line, start + 1);
        }
        if (start === end) return undefined;

        const record = parseJsonlRecord(line.slice(start, end), lineNumber);
        if (headerPending) {
            try {
                assertHeader(record);
            } catch (error) {
                throw new Error(`Invalid JSONL header at line ${lineNumber}`, { cause: error });
            }
            headerPending = false;
        }
        return record;
    };
}

/** Captures all records before asynchronous transport can observe later mutations. */
export function serializeChatPayload(payload) {
    if (!Array.isArray(payload)) {
        throw new Error('Chat payload must be an array');
    }
    if (payload.length === 0) throw new Error('Chat payload must contain a header');
    const records = [];

    for (let index = 0; index < payload.length; index += 1) {
        const record = JSON.stringify(payload[index]);
        if (record?.[0] !== '{') throw new Error(`Chat payload entry at index ${index} must serialize to an object`);
        if (index === 0) assertHeader(JSON.parse(record));
        records.push(record);
    }

    return records;
}

export function payloadToJsonl(payload) {
    return serializeChatPayload(payload).join('\n');
}

export function jsonlToPayload(text) {
    if (!text) {
        return [];
    }

    const input = String(text);
    const payload = [];
    let cursor = 0;
    const parseLine = createJsonlParser();

    while (cursor <= input.length) {
        const nextNewline = input.indexOf('\n', cursor);
        const end = nextNewline === -1 ? input.length : nextNewline;
        const line = input.slice(cursor, end);
        const parsed = parseLine(line);
        if (parsed !== undefined) payload.push(parsed);

        if (nextNewline === -1) {
            break;
        }

        cursor = nextNewline + 1;
    }

    return payload;
}

export async function visitJsonlStream(stream, visit, { hasHeader = true } = {}) {
    const reader = stream.getReader();
    // Preserve the BOM for the document parser; decoding must never repair invalid UTF-8.
    const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
    const lineFragments = [];
    const parseLine = createJsonlParser(hasHeader);

    function visitLine(rawLine) {
        const parsed = parseLine(rawLine);
        if (parsed !== undefined) visit(parsed);
    }

    function consumeText(text) {
        let start = 0;
        while (true) {
            const newlineIndex = text.indexOf('\n', start);
            if (newlineIndex === -1) {
                if (start < text.length) {
                    lineFragments.push(text.slice(start));
                }
                return;
            }

            lineFragments.push(text.slice(start, newlineIndex));
            visitLine(lineFragments.join(''));
            lineFragments.length = 0;
            start = newlineIndex + 1;
        }
    }

    try {
        while (true) {
            const { done, value } = await reader.read();
            if (done) {
                break;
            }

            consumeText(decoder.decode(value, { stream: true }));
        }

        consumeText(decoder.decode());
        if (lineFragments.length > 0) {
            visitLine(lineFragments.join(''));
        }
    } catch (error) {
        try {
            await reader.cancel();
        } catch {
            // ignore cancellation errors
        }
        throw error;
    } finally {
        reader.releaseLock();
    }
}

export async function jsonlStreamToPayload(stream, options) {
    const payload = [];
    await visitJsonlStream(stream, (entry) => payload.push(entry), options);

    return payload;
}

function concatChunks(chunks, totalLength) {
    if (chunks.length === 1) {
        return chunks[0];
    }

    const output = new Uint8Array(totalLength);
    let offset = 0;

    for (const chunk of chunks) {
        output.set(chunk, offset);
        offset += chunk.byteLength;
    }

    return output;
}

export function* jsonlRecordsToByteChunks(records, { maxChunkBytes = 4 * 1024 * 1024 } = {}) {
    if (!Number.isSafeInteger(maxChunkBytes) || maxChunkBytes <= 0) {
        throw new Error('maxChunkBytes must be a positive safe integer');
    }

    const chunks = [];
    let totalLength = 0;
    let isFirstLine = true;

    for (const line of records) {
        const text = isFirstLine ? line : `\n${line}`;
        isFirstLine = false;
        const bytes = textEncoder.encode(text);

        if (bytes.byteLength > maxChunkBytes) {
            if (totalLength > 0) {
                yield concatChunks(chunks, totalLength);
                chunks.length = 0;
                totalLength = 0;
            }

            for (let offset = 0; offset < bytes.byteLength; offset += maxChunkBytes) {
                yield bytes.subarray(offset, offset + maxChunkBytes);
            }
            continue;
        }

        if (totalLength > 0 && totalLength + bytes.byteLength > maxChunkBytes) {
            yield concatChunks(chunks, totalLength);
            chunks.length = 0;
            totalLength = 0;
        }

        chunks.push(bytes);
        totalLength += bytes.byteLength;
    }

    if (totalLength > 0) {
        yield concatChunks(chunks, totalLength);
    }
}
