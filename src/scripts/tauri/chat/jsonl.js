const textEncoder = new TextEncoder();

function trimAsciiWhitespaceRange(value) {
    let start = 0;
    let end = value.length;

    while (start < end) {
        const code = value.charCodeAt(start);
        if (code > 32) {
            break;
        }
        start += 1;
    }

    while (end > start) {
        const code = value.charCodeAt(end - 1);
        if (code > 32) {
            break;
        }
        end -= 1;
    }

    return [start, end];
}

function parseJsonlLine(line, { isFirstPayloadLine, lineNumber }) {
    const [start, end] = trimAsciiWhitespaceRange(line);
    if (end <= start) {
        return undefined;
    }

    let jsonStart = start;
    if (isFirstPayloadLine && line.charCodeAt(jsonStart) === 0xFEFF) {
        jsonStart += 1;
    }

    if (end <= jsonStart) {
        return undefined;
    }

    try {
        return JSON.parse(line.slice(jsonStart, end));
    } catch (error) {
        throw new Error(`Invalid JSONL at line ${lineNumber}`, { cause: error });
    }
}

function assertPayloadArray(payload) {
    if (!Array.isArray(payload)) {
        throw new Error('Chat payload must be an array');
    }

    return payload;
}

export function payloadToJsonl(payload) {
    const normalized = assertPayloadArray(payload);
    let result = '';

    for (let index = 0; index < normalized.length; index += 1) {
        const entry = normalized[index];
        if (!entry || typeof entry !== 'object') {
            throw new Error(`Chat payload entry at index ${index} must be an object`);
        }

        if (index > 0) {
            result += '\n';
        }
        result += JSON.stringify(entry);
    }

    return result;
}

export function jsonlToPayload(text) {
    if (!text) {
        return [];
    }

    const input = String(text);
    const payload = [];
    let cursor = 0;
    let lineNumber = 0;
    let isFirstPayloadLine = true;

    while (cursor <= input.length) {
        const nextNewline = input.indexOf('\n', cursor);
        const end = nextNewline === -1 ? input.length : nextNewline;
        const line = input.slice(cursor, end);
        lineNumber += 1;
        const parsed = parseJsonlLine(line, { isFirstPayloadLine, lineNumber });
        if (parsed !== undefined) {
            payload.push(parsed);
            isFirstPayloadLine = false;
        }

        if (nextNewline === -1) {
            break;
        }

        cursor = nextNewline + 1;
    }

    return payload;
}

export async function visitJsonlStream(stream, visit) {
    if (!stream || typeof stream.getReader !== 'function') {
        throw new Error('JSONL stream must be a ReadableStream');
    }
    if (typeof visit !== 'function') {
        throw new Error('JSONL visitor must be a function');
    }

    const reader = stream.getReader();
    const decoder = new TextDecoder();
    const lineFragments = [];
    let lineNumber = 0;
    let isFirstPayloadLine = true;

    function visitLine(rawLine) {
        lineNumber += 1;
        const parsed = parseJsonlLine(rawLine, { isFirstPayloadLine, lineNumber });
        if (parsed !== undefined) {
            visit(parsed);
            isFirstPayloadLine = false;
        }
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

export async function jsonlStreamToPayload(stream) {
    const payload = [];
    await visitJsonlStream(stream, (entry) => payload.push(entry));

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

export function* payloadToJsonlByteChunks(payload, { maxChunkBytes = 4 * 1024 * 1024 } = {}) {
    const normalized = assertPayloadArray(payload);
    if (!Number.isSafeInteger(maxChunkBytes) || maxChunkBytes <= 0) {
        throw new Error('maxChunkBytes must be a positive safe integer');
    }

    const chunks = [];
    let totalLength = 0;
    let isFirstLine = true;

    for (let index = 0; index < normalized.length; index += 1) {
        const entry = normalized[index];
        if (!entry || typeof entry !== 'object') {
            throw new Error(`Chat payload entry at index ${index} must be an object`);
        }

        const line = JSON.stringify(entry);
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
