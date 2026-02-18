import { invoke, isTauriEnv } from '../tauri-bridge.js';

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

function normalizeInvokeErrorMessage(error, fallback) {
    const extract = (value, depth = 0) => {
        if (depth > 4 || value === null || value === undefined) {
            return '';
        }

        if (typeof value === 'string') {
            return value.trim();
        }

        if (typeof value === 'number' || typeof value === 'boolean') {
            return String(value);
        }

        if (value instanceof Error) {
            const nested = extract(value.message, depth + 1);
            return nested || String(value).trim();
        }

        if (Array.isArray(value)) {
            for (const item of value) {
                const nested = extract(item, depth + 1);
                if (nested) {
                    return nested;
                }
            }
            return '';
        }

        if (typeof value === 'object') {
            const keys = ['message', 'error', 'details', 'reason', 'cause', 'data'];
            for (const key of keys) {
                if (Object.prototype.hasOwnProperty.call(value, key)) {
                    const nested = extract(value[key], depth + 1);
                    if (nested) {
                        return nested;
                    }
                }
            }
        }

        return '';
    };

    const extracted = extract(error);
    if (extracted && extracted !== '[object Object]') {
        return extracted;
    }

    try {
        const serialized = JSON.stringify(error);
        if (serialized && serialized !== '{}') {
            return serialized;
        }
    } catch {
        // Ignore serialization failures.
    }

    const stringified = String(error || '').trim();
    if (stringified && stringified !== '[object Object]') {
        return stringified;
    }

    return fallback;
}

function normalizeChatFileName(fileName) {
    const value = String(fileName || '').trim();
    if (!value) {
        return '';
    }

    return value.replace(/\.jsonl$/i, '');
}

function normalizeAvatarFileName(avatar) {
    if (avatar === null || avatar === undefined) {
        return null;
    }

    let value = String(avatar).trim();
    if (!value) {
        return null;
    }

    if (value.includes('?')) {
        try {
            const parsed = new URL(value, 'http://localhost');
            value = parsed.searchParams.get('file') || parsed.pathname || value;
        } catch {
            // Keep original value when URL parsing fails.
        }
    }

    try {
        value = decodeURIComponent(value);
    } catch {
        // Keep original value when decodeURIComponent fails.
    }

    value = value.split('?')[0].split('#')[0];
    if (!value) {
        return null;
    }

    const normalized = value.replace(/[\\/]+/g, '/');
    const fileName = normalized.split('/').pop();
    return fileName || null;
}

function getAvatarInternalId(avatar) {
    const fileName = normalizeAvatarFileName(avatar);
    if (!fileName) {
        return null;
    }

    return fileName.replace(/\.[^/.]+$/, '') || null;
}

function resolveCharacterDirectoryId(characterName, avatarUrl) {
    const fromAvatar = getAvatarInternalId(avatarUrl);
    if (fromAvatar) {
        return fromAvatar;
    }

    return String(characterName || '').trim();
}

function normalizePayload(payload) {
    if (!Array.isArray(payload)) {
        return [];
    }

    return payload.filter((entry) => entry && typeof entry === 'object');
}

export function payloadToJsonl(payload) {
    return normalizePayload(payload).map((entry) => JSON.stringify(entry)).join('\n');
}

export function jsonlToPayload(text) {
    if (!text) {
        return [];
    }

    const lines = String(text)
        .split(/\r?\n/)
        .map((line) => line.trim())
        .filter(Boolean);

    const payload = [];
    for (const line of lines) {
        try {
            payload.push(JSON.parse(line));
        } catch {
            // Ignore malformed lines to preserve existing tolerant behavior.
        }
    }

    return payload;
}

function isAndroidRuntime() {
    if (typeof navigator === 'undefined' || typeof navigator.userAgent !== 'string') {
        return false;
    }

    return /android/i.test(navigator.userAgent);
}

function normalizeBinaryPayload(value) {
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
        return Uint8Array.from(
            value.map((entry) => {
                const number = Number(entry);
                if (!Number.isFinite(number)) {
                    return 0;
                }
                return Math.max(0, Math.min(255, Math.trunc(number)));
            }),
        );
    }

    if (typeof value === 'string') {
        return textEncoder.encode(value);
    }

    return new Uint8Array();
}

function wrapTransportError(error, fallbackMessage) {
    const message = normalizeInvokeErrorMessage(error, fallbackMessage);
    const wrapped = new Error(message);
    if (String(message).toLowerCase().includes('integrity')) {
        wrapped.code = 'integrity';
    }
    return wrapped;
}

async function resolveWritableTempDir(pathApi) {
    const getAppCacheDir = typeof pathApi?.appCacheDir === 'function'
        ? () => pathApi.appCacheDir()
        : null;
    const getTempDir = typeof pathApi?.tempDir === 'function'
        ? () => pathApi.tempDir()
        : null;

    const candidates = [];
    if (isAndroidRuntime() && getAppCacheDir) {
        candidates.push(getAppCacheDir);
    }
    if (getTempDir) {
        candidates.push(getTempDir);
    }
    if (getAppCacheDir && !candidates.includes(getAppCacheDir)) {
        candidates.push(getAppCacheDir);
    }

    let lastError = null;
    for (const candidate of candidates) {
        try {
            const directory = await candidate();
            if (typeof directory === 'string' && directory.trim()) {
                return directory;
            }
        } catch (error) {
            lastError = error;
        }
    }

    if (lastError) {
        throw lastError;
    }

    throw new Error('No writable temp directory is available');
}

async function writeTempJsonlFile(jsonlText, prefix) {
    const tauri = window.__TAURI__;
    const pathApi = tauri?.path;
    const invokeApi = tauri?.core?.invoke;
    if (!pathApi || typeof pathApi.join !== 'function') {
        throw new Error('Tauri path API is unavailable');
    }
    if (typeof invokeApi !== 'function') {
        throw new Error('Tauri invoke API is unavailable');
    }

    const tempDir = await resolveWritableTempDir(pathApi);
    const fileName = `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}.jsonl`;
    const filePath = await pathApi.join(tempDir, fileName);
    const bytes = textEncoder.encode(jsonlText);

    await invokeApi('plugin:fs|write_file', bytes, {
        headers: {
            path: encodeURIComponent(filePath),
            options: '{}',
        },
    });

    return {
        filePath,
        cleanup: async () => {
            try {
                await invokeApi('plugin:fs|remove', { path: filePath });
            } catch {
                // Ignore cleanup failures.
            }
        },
    };
}

export function isTauriChatPayloadTransportEnabled() {
    return Boolean(isTauriEnv && typeof invoke === 'function');
}

export async function loadCharacterChatPayload({ characterName, avatarUrl, fileName, allowNotFound = true }) {
    const normalizedCharacter = resolveCharacterDirectoryId(characterName, avatarUrl);
    const normalizedFile = normalizeChatFileName(fileName);
    if (!normalizedCharacter || !normalizedFile) {
        return [];
    }

    try {
        if (isAndroidRuntime()) {
            const text = await invoke('get_chat_payload_text', {
                characterName: normalizedCharacter,
                fileName: normalizedFile,
                allowNotFound: Boolean(allowNotFound),
            });
            return jsonlToPayload(String(text || ''));
        }

        const rawPayload = await invoke('get_chat_payload_raw', {
            characterName: normalizedCharacter,
            fileName: normalizedFile,
            allowNotFound: Boolean(allowNotFound),
        });
        const bytes = normalizeBinaryPayload(rawPayload);
        return jsonlToPayload(textDecoder.decode(bytes));
    } catch (error) {
        const wrapped = wrapTransportError(error, 'Failed to load chat payload');
        if (allowNotFound && /not found|enoent|os error 2/i.test(wrapped.message)) {
            return [];
        }
        throw wrapped;
    }
}

export async function saveCharacterChatPayload({ characterName, avatarUrl, fileName, payload, force = false }) {
    const normalizedCharacter = resolveCharacterDirectoryId(characterName, avatarUrl);
    const normalizedFile = normalizeChatFileName(fileName);
    const normalizedPayload = normalizePayload(payload);
    if (!normalizedCharacter || !normalizedFile || normalizedPayload.length === 0) {
        throw new Error('Invalid chat payload');
    }

    const jsonlText = payloadToJsonl(normalizedPayload);

    try {
        if (isAndroidRuntime()) {
            const tempFile = await writeTempJsonlFile(jsonlText, 'tauritavern-chat');
            try {
                await invoke('save_chat_payload_from_file', {
                    dto: {
                        ch_name: normalizedCharacter,
                        file_name: normalizedFile,
                        file_path: tempFile.filePath,
                        force: Boolean(force),
                    },
                });
            } finally {
                await tempFile.cleanup();
            }
            return;
        }

        const payloadBytes = textEncoder.encode(jsonlText);
        await invoke(
            'save_chat_payload_raw',
            payloadBytes,
            {
                headers: {
                    'x-character-name': encodeURIComponent(normalizedCharacter),
                    'x-file-name': encodeURIComponent(normalizedFile),
                    'x-force': String(Boolean(force)),
                },
            },
        );
    } catch (error) {
        throw wrapTransportError(error, 'Failed to save chat payload');
    }
}

export async function loadGroupChatPayload({ id, allowNotFound = true }) {
    const normalizedId = normalizeChatFileName(id);
    if (!normalizedId) {
        return [];
    }

    try {
        if (isAndroidRuntime()) {
            const text = await invoke('get_group_chat_text', {
                dto: { id: normalizedId },
                allowNotFound: Boolean(allowNotFound),
            });
            return jsonlToPayload(String(text || ''));
        }

        const rawPayload = await invoke('get_group_chat_raw', {
            dto: { id: normalizedId },
            allowNotFound: Boolean(allowNotFound),
        });
        const bytes = normalizeBinaryPayload(rawPayload);
        return jsonlToPayload(textDecoder.decode(bytes));
    } catch (error) {
        const wrapped = wrapTransportError(error, 'Failed to load group chat payload');
        if (allowNotFound && /not found|enoent|os error 2/i.test(wrapped.message)) {
            return [];
        }
        throw wrapped;
    }
}

export async function saveGroupChatPayload({ id, payload, force = false }) {
    const normalizedId = normalizeChatFileName(id);
    const normalizedPayload = normalizePayload(payload);
    if (!normalizedId || normalizedPayload.length === 0) {
        throw new Error('Invalid group chat payload');
    }

    const jsonlText = payloadToJsonl(normalizedPayload);

    try {
        if (isAndroidRuntime()) {
            const tempFile = await writeTempJsonlFile(jsonlText, 'tauritavern-group-chat');
            try {
                await invoke('save_group_chat_from_file', {
                    dto: {
                        id: normalizedId,
                        file_path: tempFile.filePath,
                        force: Boolean(force),
                    },
                });
            } finally {
                await tempFile.cleanup();
            }
            return;
        }

        const payloadBytes = textEncoder.encode(jsonlText);
        await invoke(
            'save_group_chat_raw',
            payloadBytes,
            {
                headers: {
                    'x-chat-id': encodeURIComponent(normalizedId),
                    'x-force': String(Boolean(force)),
                },
            },
        );
    } catch (error) {
        throw wrapTransportError(error, 'Failed to save group chat payload');
    }
}
