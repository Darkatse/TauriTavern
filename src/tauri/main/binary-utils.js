const BYTE_MAX = 255;
const BASE64_PATTERN = /^[A-Za-z0-9+/]+={0,2}$/;
const textEncoder = new TextEncoder();

function clampByte(value) {
    const number = Number(value);
    if (!Number.isFinite(number)) {
        return 0;
    }

    return Math.max(0, Math.min(BYTE_MAX, Math.trunc(number)));
}

export function decodeBase64ToBytes(value) {
    const normalized = String(value || '').replace(/\s+/g, '');
    if (!normalized) {
        return new Uint8Array(0);
    }

    const binary = atob(normalized);
    const bytes = new Uint8Array(binary.length);

    for (let index = 0; index < binary.length; index += 1) {
        bytes[index] = binary.charCodeAt(index);
    }

    return bytes;
}

function tryDecodeBase64(value) {
    const normalized = String(value || '').replace(/\s+/g, '');
    if (!normalized) {
        return new Uint8Array(0);
    }

    if (normalized.length % 4 !== 0 || !BASE64_PATTERN.test(normalized)) {
        return null;
    }

    try {
        return decodeBase64ToBytes(normalized);
    } catch {
        return null;
    }
}

function decodeNumericKeyedObject(value) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        return null;
    }

    const numericKeys = Object.keys(value)
        .filter((key) => /^\d+$/.test(key))
        .sort((left, right) => Number(left) - Number(right));

    if (numericKeys.length === 0) {
        return null;
    }

    return Uint8Array.from(numericKeys.map((key) => clampByte(value[key])));
}

export function normalizeBinaryPayload(value) {
    if (value instanceof Uint8Array) {
        return value;
    }

    if (value instanceof ArrayBuffer) {
        return new Uint8Array(value);
    }

    if (ArrayBuffer.isView(value)) {
        return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    }

    if (Array.isArray(value)) {
        return Uint8Array.from(value.map(clampByte));
    }

    if (typeof value === 'string') {
        const decoded = tryDecodeBase64(value);
        return decoded ?? textEncoder.encode(value);
    }

    if (value && typeof value === 'object') {
        if (Array.isArray(value.data)) {
            return Uint8Array.from(value.data.map(clampByte));
        }

        const keyed = decodeNumericKeyedObject(value);
        if (keyed) {
            return keyed;
        }
    }

    return new Uint8Array(0);
}

export function sanitizeAttachmentFileName(value, fallback = 'download.bin') {
    const fallbackName = String(fallback || 'download.bin').trim() || 'download.bin';
    const fileName = String(value || '').trim() || fallbackName;
    const sanitized = fileName
        .replace(/[\/\\:*?"<>|\u0000-\u001f]/g, '_')
        .replace(/[\r\n]+/g, ' ')
        .replace(/[. ]+$/g, '')
        .trim();

    return sanitized || fallbackName;
}
