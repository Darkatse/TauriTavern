// @ts-check

/** @param {string} fileName */
export function ensureJsonl(fileName) {
    const value = String(fileName || '');
    if (!value) {
        return value;
    }

    return value.endsWith('.jsonl') ? value : `${value}.jsonl`;
}

/** @param {string} fileName */
export function stripJsonl(fileName) {
    const value = String(fileName || '').trim();
    if (!value) {
        return '';
    }

    return value.replace(/\.jsonl$/i, '');
}

/**
 * @param {any} chatDto
 */
export function toFrontendChat(chatDto) {
    if (Array.isArray(chatDto)) {
        return chatDto.map((entry) => (entry && typeof entry === 'object' ? { ...entry } : entry));
    }

    /** @type {any[]} */
    const rawMessages = Array.isArray(chatDto?.messages) ? chatDto.messages : [];

    const messages = rawMessages
        .map((message) => {
            const messageAdditional = message?.additional && typeof message.additional === 'object'
                ? message.additional
                : {};
            const rawExtra = message?.extra && typeof message.extra === 'object' ? message.extra : {};
            const extraAdditional = rawExtra?.additional && typeof rawExtra.additional === 'object'
                ? rawExtra.additional
                : {};
            const extra = { ...rawExtra, ...extraAdditional };
            delete extra.additional;

            return {
                ...messageAdditional,
                name: message.name,
                is_user: Boolean(message.is_user),
                is_system: Boolean(message.is_system),
                send_date: message.send_date,
                mes: message.mes,
                extra,
            };
        });

    const metadata = chatDto?.chat_metadata && typeof chatDto.chat_metadata === 'object'
        ? chatDto.chat_metadata
        : { chat_id_hash: Number(chatDto?.chat_id || 0) };

    const header = {
        user_name: chatDto?.user_name || 'User',
        character_name: chatDto?.character_name || '',
        create_date: chatDto?.create_date || '',
        chat_metadata: metadata,
    };

    return [header, ...messages];
}

/**
 * @param {any} value
 */
export function formatFileSize(value) {
    const size = Number(value || 0);
    if (!Number.isFinite(size) || size <= 0) {
        return '0 B';
    }

    const units = ['B', 'KB', 'MB', 'GB'];
    let unitIndex = 0;
    let result = size;

    while (result >= 1024 && unitIndex < units.length - 1) {
        result /= 1024;
        unitIndex += 1;
    }

    return `${result.toFixed(result >= 10 ? 0 : 1)} ${units[unitIndex]}`;
}

/**
 * @param {number} epoch
 */
function normalizeEpochMillis(epoch) {
    if (!Number.isFinite(epoch)) {
        return 0;
    }

    const normalized = Math.trunc(epoch);
    return Math.abs(normalized) < 1_000_000_000_000 ? normalized * 1000 : normalized;
}

/**
 * @param {any} sendDate
 */
export function parseTimestamp(sendDate) {
    if (typeof sendDate === 'number') {
        return normalizeEpochMillis(sendDate);
    }

    const raw = String(sendDate || '').trim();
    if (!raw) {
        return 0;
    }

    if (/^-?\d+(\.\d+)?$/.test(raw)) {
        return normalizeEpochMillis(Number(raw));
    }

    const parsed = Date.parse(raw);
    return Number.isFinite(parsed) ? parsed : 0;
}

/**
 * @param {any} frontendChat
 */
export function exportChatAsText(frontendChat) {
    const payload = Array.isArray(frontendChat) ? frontendChat : [];
    const lines = payload
        .slice(1)
        .filter((message) => !Boolean(message?.is_system))
        .map((message) => {
            const role = message?.name || (message?.is_user ? 'User' : 'Assistant');
            const displayText = message?.extra?.display_text || message?.mes || '';
            return `${role}: ${String(displayText).replace(/\r?\n/g, '\n')}`;
        });

    return lines.join('\n\n');
}

/**
 * @param {any[]} frontendChat
 */
export function exportChatAsJsonl(frontendChat) {
    return frontendChat.map((item) => JSON.stringify(item)).join('\n');
}
