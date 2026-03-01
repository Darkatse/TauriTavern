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

export async function jsonlStreamToPayload(stream) {
    if (!stream || typeof stream.getReader !== 'function') {
        throw new Error('JSONL stream must be a ReadableStream');
    }

    const reader = stream.getReader();
    const decoder = new TextDecoder();
    const payload = [];
    let carry = '';
    let lineNumber = 0;
    let isFirstPayloadLine = true;

    try {
        while (true) {
            const { done, value } = await reader.read();
            if (done) {
                break;
            }

            carry += decoder.decode(value, { stream: true });

            while (true) {
                const newlineIndex = carry.indexOf('\n');
                if (newlineIndex === -1) {
                    break;
                }

                const rawLine = carry.slice(0, newlineIndex);
                carry = carry.slice(newlineIndex + 1);

                lineNumber += 1;
                const parsed = parseJsonlLine(rawLine, { isFirstPayloadLine, lineNumber });
                if (parsed !== undefined) {
                    payload.push(parsed);
                    isFirstPayloadLine = false;
                }
            }
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

    carry += decoder.decode();
    if (carry) {
        lineNumber += 1;
        const parsed = parseJsonlLine(carry, { isFirstPayloadLine, lineNumber });
        if (parsed !== undefined) {
            payload.push(parsed);
        }
    }

    return payload;
}

function concatChunks(chunks, totalLength) {
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
